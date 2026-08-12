use std::env;
use std::ffi::OsString;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
use std::path::Path;
use std::time::Duration;

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::platform::{
    CommandOutput, CommandRunner, Platform, ProcessCommandRunner, ServiceStatus,
};
use crate::storage::filesystem::RuntimeStore;
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
            Err(command_failure("Windows 服务管理失败", output))
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
            "  $acl = $item.GetAccessControl()\n",
            "  $acl.SetAccessRuleProtection($true, $false)\n",
            "  $inheritance = if ($item.PSIsContainer) { [Security.AccessControl.InheritanceFlags]::ContainerInherit -bor [Security.AccessControl.InheritanceFlags]::ObjectInherit } else { [Security.AccessControl.InheritanceFlags]::None }\n",
            "  foreach ($identity in @($system, $administrators)) {\n",
            "    $rule = [Security.AccessControl.FileSystemAccessRule]::new($identity, [Security.AccessControl.FileSystemRights]::FullControl, $inheritance, [Security.AccessControl.PropagationFlags]::None, [Security.AccessControl.AccessControlType]::Allow)\n",
            "    $acl.SetAccessRule($rule)\n",
            "  }\n",
            "  $item.SetAccessControl($acl)\n",
            "}\n"
        );
        self.run_powershell_required(script, vec![self.paths.root.clone().into_os_string()])
    }

    fn register_current_task(&self, service: Service, release: &Path) -> Result<(), AppError> {
        let definition = ServiceCatalog::definition(service);
        let binary = release.join(definition.windows_binary_name);
        if !binary.is_file() {
            return Err(AppError::State(format!(
                "当前版本缺少 Windows 服务二进制：{}",
                binary.display()
            )));
        }
        let config = match service {
            Service::Cli => self.paths.config.join("config.yaml"),
            Service::Keeper => self.paths.config.join("keeper.env"),
        };
        let argument = match service {
            Service::Cli => "-config",
            Service::Keeper => "-env",
        };
        let out_log = self
            .paths
            .logs
            .join(format!("{}.out.log", definition.log_prefix));
        let err_log = self
            .paths
            .logs
            .join(format!("{}.err.log", definition.log_prefix));
        let script = concat!(
            "param([string]$TaskName, [string]$Binary, [string]$Config, [string]$OutLog, [string]$ErrLog, [string]$ServiceArgument)\n",
            "$ErrorActionPreference = 'Stop'\n",
            "$command = '\"\"' + $Binary + '\" ' + $ServiceArgument + ' \"' + $Config + '\" 1>> \"' + $OutLog + '\" 2>> \"' + $ErrLog + '\"\"'\n",
            "$action = New-ScheduledTaskAction -Execute $env:ComSpec -Argument ('/D /S /C ' + $command)\n",
            "$trigger = New-ScheduledTaskTrigger -AtStartup\n",
            "$settings = New-ScheduledTaskSettingsSet -RestartCount 999 -RestartInterval (New-TimeSpan -Minutes 1) -ExecutionTimeLimit (New-TimeSpan -Days 0)\n",
            "Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Settings $settings -User 'SYSTEM' -RunLevel Highest -Force | Out-Null\n"
        );
        self.run_powershell_required(
            script,
            vec![
                OsString::from(Self::task_name(service)),
                binary.into_os_string(),
                config.into_os_string(),
                out_log.into_os_string(),
                err_log.into_os_string(),
                OsString::from(argument),
            ],
        )
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
    let raw_diagnostic = if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    };
    if raw_diagnostic.is_empty() {
        AppError::Service(format!(
            "{prefix}：系统命令未返回可用诊断。请执行 cpactl doctor 检查环境。"
        ))
    } else {
        AppError::ServiceDiagnostic {
            message: format!(
                "{prefix}：{}。请以管理员身份运行 cpactl，并执行 cpactl doctor 检查环境。",
                windows_failure_reason(&raw_diagnostic)
            ),
            raw_diagnostic,
        }
    }
}

fn windows_failure_reason(raw_diagnostic: &str) -> &'static str {
    let diagnostic = raw_diagnostic.to_ascii_lowercase();
    if diagnostic.contains("access is denied") || raw_diagnostic.contains("拒绝访问") {
        "访问被拒绝"
    } else if diagnostic.contains("cannot find")
        || diagnostic.contains("not found")
        || raw_diagnostic.contains("找不到")
    {
        "找不到所需的系统资源"
    } else if diagnostic.contains("parameter") || raw_diagnostic.contains("参数") {
        "系统命令参数无效"
    } else {
        "系统命令执行失败"
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
        self.remove_firewall()
    }

    fn start(&self, service: Service) -> Result<(), AppError> {
        self.clear_disabled(service)?;
        self.run_powershell_required(
            "param([string]$TaskName) Enable-ScheduledTask -TaskName $TaskName | Out-Null; Start-ScheduledTask -TaskName $TaskName",
            vec![OsString::from(Self::task_name(service))],
        )
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        self.mark_disabled(service)?;
        self.run_powershell_required(
            "param([string]$TaskName) Disable-ScheduledTask -TaskName $TaskName | Out-Null; Stop-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue",
            vec![OsString::from(Self::task_name(service))],
        )
    }

    fn restart(&self, service: Service) -> Result<(), AppError> {
        self.start(service)
    }

    fn status(&self, service: Service) -> Result<ServiceStatus, AppError> {
        let output = self.run_powershell(
            concat!(
                "param([string]$TaskName)\n",
                "$managed = @(Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue).Count -gt 0\n",
                "Write-Output \"$([int]$managed)\"\n"
            ),
            vec![OsString::from(Self::task_name(service))],
        )?;
        let managed = output.success && output.stdout.trim() != "0";
        Ok(ServiceStatus {
            managed,
            disabled: self.paths.disabled_file(service).exists(),
            listening: port_listening(service),
        })
    }

    fn statuses(&self) -> Result<[(Service, ServiceStatus); 2], AppError> {
        let output = self.run_powershell(
            concat!(
                "param([string]$CliTask, [string]$KeeperTask)\n",
                "$cli = @(Get-ScheduledTask -TaskName $CliTask -ErrorAction SilentlyContinue).Count -gt 0\n",
                "$keeper = @(Get-ScheduledTask -TaskName $KeeperTask -ErrorAction SilentlyContinue).Count -gt 0\n",
                "Write-Output \"$([int]$cli),$([int]$keeper)\"\n"
            ),
            vec![
                OsString::from(Self::task_name(Service::Cli)),
                OsString::from(Self::task_name(Service::Keeper)),
            ],
        )?;
        let (cli_managed, keeper_managed) = parse_managed_statuses(&output);
        let [cli_listening, keeper_listening] = parallel_port_statuses();
        Ok([
            (
                Service::Cli,
                ServiceStatus {
                    managed: cli_managed,
                    disabled: self.paths.disabled_file(Service::Cli).exists(),
                    listening: cli_listening,
                },
            ),
            (
                Service::Keeper,
                ServiceStatus {
                    managed: keeper_managed,
                    disabled: self.paths.disabled_file(Service::Keeper).exists(),
                    listening: keeper_listening,
                },
            ),
        ])
    }

    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        RuntimeStore::new(self.paths.clone()).set_current(service, release)?;
        self.register_current_task(service, release)
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

fn parse_managed_statuses(output: &CommandOutput) -> (bool, bool) {
    if !output.success {
        return (false, false);
    }
    match output.stdout.split_once(',') {
        Some((managed, listening)) => (managed.trim() == "1", listening.trim() == "1"),
        None => (true, true),
    }
}

fn port_listening(service: Service) -> bool {
    let address = SocketAddr::new(
        IpAddr::V4(Ipv4Addr::LOCALHOST),
        ServiceCatalog::definition(service).port,
    );
    TcpStream::connect_timeout(&address, Duration::from_millis(250)).is_ok()
}

fn parallel_port_statuses() -> [bool; 2] {
    std::thread::scope(|scope| {
        let cli = scope.spawn(|| port_listening(Service::Cli));
        let keeper = scope.spawn(|| port_listening(Service::Keeper));
        [cli.join().unwrap_or(false), keeper.join().unwrap_or(false)]
    })
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
