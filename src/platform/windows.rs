use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::platform::{
    CommandOutput, CommandRunner, Platform, ProcessCommandRunner, ServiceStatus,
};
use base64::Engine;

const FIREWALL_RULE: &str = "CPAStack-Block-Remote-Keeper";

#[derive(Clone, Debug)]
pub struct WindowsPlatform<R = ProcessCommandRunner> {
    runner: R,
    paths: RuntimePaths,
}

impl WindowsPlatform<ProcessCommandRunner> {
    pub const fn new(paths: RuntimePaths) -> Self {
        Self {
            runner: ProcessCommandRunner,
            paths,
        }
    }
}

impl<R: CommandRunner> WindowsPlatform<R> {
    pub const fn with_runner(runner: R, paths: RuntimePaths) -> Self {
        Self { runner, paths }
    }

    fn task_name(service: Service) -> &'static str {
        match service {
            Service::Cli => "CPAStack-CLIProxyAPI",
            Service::Keeper => "CPAStack-UsageKeeper",
        }
    }

    fn wrapper_path(&self, service: Service) -> PathBuf {
        self.paths.tasks.join(match service {
            Service::Cli => "run-cli-proxy-api.ps1",
            Service::Keeper => "run-cpa-usage-keeper.ps1",
        })
    }

    fn run_powershell(
        &self,
        script: &str,
        parameters: Vec<OsString>,
    ) -> Result<CommandOutput, AppError> {
        let args = vec![
            OsString::from("-NoProfile"),
            OsString::from("-NonInteractive"),
            OsString::from("-ExecutionPolicy"),
            OsString::from("Bypass"),
            OsString::from("-EncodedCommand"),
            OsString::from(encode_powershell_invocation(script, &parameters)?),
        ];
        self.runner.run("powershell.exe", &args)
    }

    fn run_powershell_required(
        &self,
        script: &str,
        parameters: Vec<OsString>,
    ) -> Result<(), AppError> {
        let output = self.run_powershell(script, parameters)?;
        if output.success {
            Ok(())
        } else {
            Err(command_failure("Windows 服务管理命令执行失败", output))
        }
    }

    fn set_root_acl(&self) -> Result<(), AppError> {
        let script = concat!(
            "param([string]$Root)\n",
            "$ErrorActionPreference = 'Stop'\n",
            "$system = [Security.Principal.SecurityIdentifier]::new('S-1-5-18')\n",
            "$administrators = [Security.Principal.SecurityIdentifier]::new('S-1-5-32-544')\n",
            "$items = @((Get-Item -LiteralPath $Root -Force)) + @(Get-ChildItem -LiteralPath $Root -Force -Recurse)\n",
            "foreach ($item in $items) {\n",
            "  $acl = Get-Acl -LiteralPath $item.FullName\n",
            "  $acl.SetAccessRuleProtection($true, $false)\n",
            "  $inheritance = if ($item.PSIsContainer) { [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit } else { [Security.AccessControl.InheritanceFlags]::None }\n",
            "  foreach ($identity in @($system, $administrators)) {\n",
            "    $rule = [Security.AccessControl.FileSystemAccessRule]::new($identity, [Security.AccessControl.FileSystemRights]::FullControl, $inheritance, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)\n",
            "    $acl.SetAccessRule($rule)\n",
            "  }\n",
            "  Set-Acl -LiteralPath $item.FullName -AclObject $acl\n",
            "}\n"
        );
        self.run_powershell_required(script, vec![self.paths.root.clone().into_os_string()])
    }

    fn write_wrapper(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.tasks)
            .map_err(|_| AppError::State("无法创建 Windows 服务任务目录".into()))?;
        let definition = ServiceCatalog::definition(service);
        let config = match service {
            Service::Cli => "config\\config.yaml",
            Service::Keeper => "config\\keeper.env",
        };
        let argument = match service {
            Service::Cli => "-config",
            Service::Keeper => "-env",
        };
        // The runtime root is supplied by the scheduled task as a PowerShell parameter;
        // no user-controlled path is interpolated into this script.
        let wrapper = format!(
            concat!(
                "param([Parameter(Mandatory=$true)][string]$Root)\n",
                "$ErrorActionPreference = 'Stop'\n",
                "$disabled = Join-Path $Root 'state\\{service}.disabled'\n",
                "if (Test-Path -LiteralPath $disabled) {{ exit 0 }}\n",
                "$binary = Join-Path $Root 'current\\{service}\\{binary}'\n",
                "$config = Join-Path $Root '{config}'\n",
                "$outLog = Join-Path $Root 'logs\\{log}.out.log'\n",
                "$errLog = Join-Path $Root 'logs\\{log}.err.log'\n",
                "& $binary {argument} $config 1>> $outLog 2>> $errLog\n"
            ),
            service = service.key(),
            binary = definition.windows_binary_name,
            config = config,
            log = definition.log_prefix,
            argument = argument,
        );
        let path = self.wrapper_path(service);
        if fs::read_to_string(&path)
            .ok()
            .as_deref()
            .is_some_and(|existing| existing == wrapper)
        {
            return Ok(());
        }
        fs::write(&path, wrapper).map_err(|error| {
            AppError::State(format!(
                "无法写入 Windows 服务包装器 {}：{error}",
                path.display()
            ))
        })
    }

    fn clear_disabled(&self, service: Service) -> Result<(), AppError> {
        match fs::remove_file(self.paths.disabled_file(service)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::State("无法清除服务停用标记".into())),
        }
    }

    fn mark_disabled(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.state)
            .map_err(|_| AppError::State("无法创建服务状态目录".into()))?;
        fs::write(self.paths.disabled_file(service), b"disabled\n")
            .map_err(|_| AppError::State("无法写入服务停用标记".into()))
    }
}

fn command_failure(prefix: &str, output: CommandOutput) -> AppError {
    if output.stderr.is_empty() {
        AppError::Service(prefix.into())
    } else {
        AppError::Service(format!("{prefix}：{}", output.stderr))
    }
}

impl<R: CommandRunner> Platform for WindowsPlatform<R> {
    fn check_supported(&self) -> Result<(), AppError> {
        if env::consts::OS == "windows" && env::consts::ARCH == "x86_64" {
            Ok(())
        } else {
            Err(AppError::Usage("仅支持 Windows x64".into()))
        }
    }

    fn check_permissions(&self) -> Result<(), AppError> {
        let script = concat!(
            "$identity = [Security.Principal.WindowsIdentity]::GetCurrent(); ",
            "$principal = [Security.Principal.WindowsPrincipal]::new($identity); ",
            "if ($principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) { exit 0 }; exit 1"
        );
        if self.run_powershell(script, vec![])?.success {
            Ok(())
        } else {
            Err(AppError::Permission(
                "请在提升权限的管理员 PowerShell 中运行 cpactl".into(),
            ))
        }
    }

    fn install_services(&self) -> Result<(), AppError> {
        for path in [
            &self.paths.root,
            &self.paths.logs,
            &self.paths.state,
            &self.paths.current,
            &self.paths.tasks,
        ] {
            fs::create_dir_all(path)
                .map_err(|_| AppError::State("无法创建 Windows 运行目录".into()))?;
        }
        self.set_root_acl()?;
        self.write_wrapper(Service::Cli)?;
        self.write_wrapper(Service::Keeper)?;

        let script = concat!(
            "param([string]$Root, [string]$CliWrapper, [string]$KeeperWrapper)\n",
            "$ErrorActionPreference = 'Stop'\n",
            "function Quote-TaskArgument([string]$Value) { \"'\" + $Value.Replace(\"'\", \"''\") + \"'\" }\n",
            "$wrappers = @{ 'CPAStack-CLIProxyAPI' = $CliWrapper; 'CPAStack-UsageKeeper' = $KeeperWrapper }\n",
            "foreach ($entry in $wrappers.GetEnumerator()) {\n",
            "  $arguments = '-NoProfile -NonInteractive -ExecutionPolicy Bypass -File ' + (Quote-TaskArgument $entry.Value) + ' -Root ' + (Quote-TaskArgument $Root)\n",
            "  $action = New-ScheduledTaskAction -Execute 'powershell.exe' -Argument $arguments\n",
            "  $trigger = New-ScheduledTaskTrigger -AtStartup\n",
            "  $settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit (New-TimeSpan -Days 0)\n",
            "  Register-ScheduledTask -TaskName $entry.Key -Action $action -Trigger $trigger -Settings $settings -User 'SYSTEM' -RunLevel Highest -Force | Out-Null\n",
            "}\n"
        );
        self.run_powershell_required(
            script,
            vec![
                self.paths.root.clone().into_os_string(),
                self.wrapper_path(Service::Cli).into_os_string(),
                self.wrapper_path(Service::Keeper).into_os_string(),
            ],
        )?;
        self.configure_firewall()
    }

    fn remove_services(&self) -> Result<(), AppError> {
        let script = concat!(
            "param([string]$CliTask, [string]$KeeperTask)\n",
            "Unregister-ScheduledTask -TaskName $CliTask -Confirm:$false -ErrorAction SilentlyContinue\n",
            "Unregister-ScheduledTask -TaskName $KeeperTask -Confirm:$false -ErrorAction SilentlyContinue\n"
        );
        self.run_powershell_required(
            script,
            vec![
                OsString::from(Self::task_name(Service::Cli)),
                OsString::from(Self::task_name(Service::Keeper)),
            ],
        )?;
        remove_if_exists(&self.wrapper_path(Service::Cli))?;
        remove_if_exists(&self.wrapper_path(Service::Keeper))?;
        self.remove_firewall()
    }

    fn start(&self, service: Service) -> Result<(), AppError> {
        self.clear_disabled(service)?;
        self.run_powershell_required(
            "param([string]$TaskName) Start-ScheduledTask -TaskName $TaskName",
            vec![OsString::from(Self::task_name(service))],
        )
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        self.mark_disabled(service)?;
        self.run_powershell_required(
            "param([string]$TaskName) Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue",
            vec![OsString::from(Self::task_name(service))],
        )
    }

    fn restart(&self, service: Service) -> Result<(), AppError> {
        self.start(service)
    }

    fn status(&self, service: Service) -> Result<ServiceStatus, AppError> {
        let port = ServiceCatalog::definition(service).port;
        let output = self.run_powershell(
            concat!(
                "param([string]$TaskName, [int]$Port)\n",
                "$managed = @(Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue).Count -gt 0\n",
                "$listening = @(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue).Count -gt 0\n",
                "Write-Output \"$([int]$managed),$([int]$listening)\"\n"
            ),
            vec![
                OsString::from(Self::task_name(service)),
                OsString::from(port.to_string()),
            ],
        )?;
        let (managed, listening) = parse_status(&output);
        Ok(ServiceStatus {
            managed,
            disabled: self.paths.disabled_file(service).exists(),
            listening,
        })
    }

    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        if !release.is_dir() {
            return Err(AppError::State("待激活版本目录不存在".into()));
        }
        fs::create_dir_all(&self.paths.current)
            .map_err(|_| AppError::State("无法创建当前版本目录".into()))?;
        let current = self.paths.current.join(service.key());
        let temporary = self.paths.current.join(format!("{}.next", service.key()));
        let previous = self
            .paths
            .current
            .join(format!("{}.previous", service.key()));
        let script = concat!(
            "param([string]$Current, [string]$Temporary, [string]$Previous, [string]$Release)\n",
            "$ErrorActionPreference = 'Stop'\n",
            "function Remove-Junction([string]$Path) { if (Test-Path -LiteralPath $Path) { [System.IO.Directory]::Delete($Path) } }\n",
            "Remove-Junction $Temporary\n",
            "Remove-Junction $Previous\n",
            "New-Item -ItemType Junction -Path $Temporary -Target $Release | Out-Null\n",
            "if (Test-Path -LiteralPath $Current) { Move-Item -LiteralPath $Current -Destination $Previous -Force }\n",
            "Move-Item -LiteralPath $Temporary -Destination $Current -Force\n"
        );
        self.run_powershell_required(
            script,
            vec![
                current.into_os_string(),
                temporary.into_os_string(),
                previous.into_os_string(),
                release.to_path_buf().into_os_string(),
            ],
        )
    }

    fn is_port_listening(&self, service: Service) -> Result<bool, AppError> {
        let port = ServiceCatalog::definition(service).port;
        Ok(self
            .run_powershell(
                concat!(
                    "param([int]$Port) ",
                    "if (@(Get-NetTCPConnection -LocalPort $Port -State Listen -ErrorAction SilentlyContinue).Count -gt 0) { exit 0 }; exit 1"
                ),
                vec![OsString::from(port.to_string())],
            )?
            .success)
    }

    fn configure_firewall(&self) -> Result<(), AppError> {
        let script = format!(
            "Remove-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -ErrorAction SilentlyContinue\nNew-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -Direction Inbound -Action Block -Protocol TCP -LocalPort 18080 -Profile Any | Out-Null\n"
        );
        self.run_powershell_required(&script, vec![])
    }
}

impl<R: CommandRunner> WindowsPlatform<R> {
    fn remove_firewall(&self) -> Result<(), AppError> {
        let script = format!(
            "Remove-NetFirewallRule -DisplayName '{FIREWALL_RULE}' -ErrorAction SilentlyContinue"
        );
        self.run_powershell_required(&script, vec![])
    }
}

fn encode_powershell(value: &str) -> String {
    let utf16: Vec<u8> = value.encode_utf16().flat_map(u16::to_le_bytes).collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

fn parse_status(output: &CommandOutput) -> (bool, bool) {
    if !output.success {
        return (false, false);
    }
    match output.stdout.split_once(',') {
        Some((managed, listening)) => (managed.trim() == "1", listening.trim() == "1"),
        None => (true, true),
    }
}

fn encode_powershell_invocation(script: &str, parameters: &[OsString]) -> Result<String, AppError> {
    if parameters.is_empty() {
        return Ok(encode_powershell(script));
    }

    let parameters = parameters
        .iter()
        .map(|parameter| {
            parameter
                .to_str()
                .map(str::to_owned)
                .ok_or_else(|| AppError::Usage("Windows 运行目录必须是有效的 Unicode 路径".into()))
        })
        .collect::<Result<Vec<_>, _>>()?;
    let serialized = serde_json::to_string(&parameters)
        .map_err(|error| AppError::State(format!("无法编码 Windows 服务参数：{error}")))?;
    let payload = encode_powershell(&serialized);
    let invocation = format!(
        concat!(
            "$cpactlParameters = [Text.Encoding]::Unicode.GetString([Convert]::FromBase64String('{payload}')) | ConvertFrom-Json\n",
            "& {{\n{script}\n}} @cpactlParameters"
        ),
        payload = payload,
        script = script
    );
    Ok(encode_powershell(&invocation))
}

fn remove_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::State("无法移除 Windows 服务包装器".into())),
    }
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    #[test]
    fn encoded_invocation_binds_each_runtime_parameter() {
        let temporary = tempfile::tempdir().unwrap();
        let paths = RuntimePaths::from_root(temporary.path().to_path_buf()).unwrap();
        let platform = WindowsPlatform::with_runner(ProcessCommandRunner, paths);

        let output = platform
            .run_powershell(
                concat!(
                    "param([string]$First, [string]$Second)\n",
                    "if ($First -eq 'first' -and $Second -eq 'second') { exit 0 }\n",
                    "exit 1\n"
                ),
                vec![OsString::from("first"), OsString::from("second")],
            )
            .unwrap();

        assert!(output.success, "{}", output.stderr);
    }
}
