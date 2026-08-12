use std::collections::VecDeque;
use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use base64::Engine;
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
#[cfg(debug_assertions)]
use cpactl::platform::ProcessCommandRunner;
use cpactl::platform::{CommandOutput, CommandRunner, MacosPlatform, Platform, WindowsPlatform};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct RecordingRunner {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
    results: Arc<Mutex<VecDeque<CommandOutput>>>,
}

impl RecordingRunner {
    fn with_results(results: impl IntoIterator<Item = CommandOutput>) -> Self {
        Self {
            calls: Arc::default(),
            results: Arc::new(Mutex::new(results.into_iter().collect())),
        }
    }

    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }

    fn scripts(&self) -> Vec<String> {
        self.calls()
            .into_iter()
            .filter(|call| {
                call.first()
                    .is_some_and(|program| program == "powershell.exe")
            })
            .filter_map(|call| powershell_script(&call))
            .collect()
    }
}

fn powershell_script(call: &[String]) -> Option<String> {
    powershell_value(call, "-EncodedCommand")
}

fn powershell_arguments(call: &[String]) -> Option<String> {
    powershell_value(call, "-EncodedArguments")
}

fn powershell_parameters(call: &[String]) -> Option<Vec<String>> {
    let script = powershell_script(call)?;
    let prefix = "FromBase64String('";
    let encoded = script.split(prefix).nth(1)?.split("')").next()?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let utf16 = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    serde_json::from_str(&String::from_utf16(&utf16).ok()?).ok()
}

fn powershell_value(call: &[String], flag: &str) -> Option<String> {
    let encoded = call.windows(2).find(|args| args[0] == flag)?.get(1)?;
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .ok()?;
    let utf16 = bytes
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect::<Vec<_>>();
    String::from_utf16(&utf16).ok()
}

#[test]
#[cfg(debug_assertions)]
fn debug_smoke_environment_does_not_execute_platform_commands() {
    temp_env::with_var("CPACTL_SMOKE_NO_PLATFORM_COMMANDS", Some("1"), || {
        let output = ProcessCommandRunner
            .run("cpactl-smoke-command-must-not-run", &[])
            .unwrap();

        assert!(output.success);
    });
}

impl CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[OsString]) -> Result<CommandOutput, AppError> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
        self.calls.lock().unwrap().push(call);
        Ok(self
            .results
            .lock()
            .unwrap()
            .pop_front()
            .unwrap_or_else(CommandOutput::success))
    }
}

fn paths(temp_dir: &TempDir) -> RuntimePaths {
    RuntimePaths::from_root(PathBuf::from(temp_dir.path())).unwrap()
}

fn macos_domain() -> String {
    let output = std::process::Command::new("id").arg("-u").output().unwrap();
    let user_id = std::str::from_utf8(&output.stdout)
        .unwrap()
        .trim()
        .parse::<u32>()
        .unwrap();
    format!("gui/{user_id}")
}

fn macos_target(service: Service) -> String {
    format!(
        "{}/{}",
        macos_domain(),
        cpactl::domain::service::ServiceCatalog::definition(service).launchd_label
    )
}

#[test]
fn macos_start_clears_marker_then_kickstarts_launchagent() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    std::fs::create_dir_all(&paths.state).unwrap();
    std::fs::write(paths.disabled_file(Service::Cli), "disabled").unwrap();
    let runner = RecordingRunner::default();
    let platform = MacosPlatform::with_runner(runner.clone(), paths.clone());

    platform.start(Service::Cli).unwrap();

    assert_eq!(
        runner.calls(),
        vec![vec![
            "launchctl".to_owned(),
            "kickstart".to_owned(),
            "-k".to_owned(),
            macos_target(Service::Cli),
        ]]
    );
    assert!(!paths.disabled_file(Service::Cli).exists());
}

#[test]
fn macos_stop_marks_service_disabled_before_killing_launchagent() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::default();
    let platform = MacosPlatform::with_runner(runner.clone(), paths.clone());

    platform.stop(Service::Keeper).unwrap();

    assert!(paths.disabled_file(Service::Keeper).exists());
    assert_eq!(
        runner.calls(),
        vec![vec![
            "launchctl".to_owned(),
            "kill".to_owned(),
            "SIGTERM".to_owned(),
            macos_target(Service::Keeper),
        ]]
    );
}

#[test]
fn macos_install_skips_bootstrap_for_already_loaded_launchagents() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::with_results([
        CommandOutput::success(),
        CommandOutput::success(),
        CommandOutput::success(),
        CommandOutput::success(),
    ]);
    let platform = MacosPlatform::with_runner(runner.clone(), paths);

    platform.install_services().unwrap();

    assert_eq!(
        runner.calls(),
        vec![
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Cli)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Cli),
            ],
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Keeper)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Keeper),
            ],
        ]
    );
}

#[test]
fn macos_install_boots_out_attempted_services_after_registration_failure() {
    let temp_dir = TempDir::new().unwrap();
    let runner = RecordingRunner::with_results([
        CommandOutput::success(),
        CommandOutput::failure(),
        CommandOutput::success(),
        CommandOutput::success(),
        CommandOutput::failure(),
        CommandOutput::failure(),
        CommandOutput::success(),
        CommandOutput::success(),
    ]);
    let platform = MacosPlatform::with_runner(runner.clone(), paths(&temp_dir));

    assert!(platform.install_services().is_err());

    assert_eq!(
        runner.calls(),
        vec![
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Cli)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Cli),
            ],
            vec![
                "launchctl".to_owned(),
                "bootstrap".to_owned(),
                macos_domain(),
                platform
                    .plist_path(Service::Cli)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Keeper)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Keeper),
            ],
            vec![
                "launchctl".to_owned(),
                "bootstrap".to_owned(),
                macos_domain(),
                platform
                    .plist_path(Service::Keeper)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "bootout".to_owned(),
                macos_target(Service::Keeper),
            ],
            vec![
                "launchctl".to_owned(),
                "bootout".to_owned(),
                macos_target(Service::Cli),
            ],
        ]
    );
}

#[test]
fn macos_install_writes_validated_plists_and_service_wrappers() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::with_results([
        CommandOutput::success(),
        CommandOutput::failure(),
        CommandOutput::success(),
        CommandOutput::success(),
        CommandOutput::failure(),
        CommandOutput::success(),
    ]);
    let platform = MacosPlatform::with_runner(runner.clone(), paths.clone());

    platform.install_services().unwrap();

    let cli_wrapper = std::fs::read_to_string(paths.bin.join("run-cli-proxy-api")).unwrap();
    assert!(cli_wrapper.contains("-config"));
    assert!(cli_wrapper.contains("config.yaml"));
    assert_eq!(
        runner.calls(),
        vec![
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Cli)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Cli),
            ],
            vec![
                "launchctl".to_owned(),
                "bootstrap".to_owned(),
                macos_domain(),
                platform
                    .plist_path(Service::Cli)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "plutil".to_owned(),
                "-lint".to_owned(),
                platform
                    .plist_path(Service::Keeper)
                    .to_string_lossy()
                    .into_owned(),
            ],
            vec![
                "launchctl".to_owned(),
                "print".to_owned(),
                macos_target(Service::Keeper),
            ],
            vec![
                "launchctl".to_owned(),
                "bootstrap".to_owned(),
                macos_domain(),
                platform
                    .plist_path(Service::Keeper)
                    .to_string_lossy()
                    .into_owned(),
            ],
        ]
    );
}

#[test]
fn macos_port_health_uses_lsof_with_the_service_port_as_an_argument() {
    let temp_dir = TempDir::new().unwrap();
    let runner = RecordingRunner::default();
    let platform = MacosPlatform::with_runner(runner.clone(), paths(&temp_dir));

    assert!(platform.is_port_listening(Service::Cli).unwrap());

    assert_eq!(
        runner.calls(),
        vec![vec![
            "lsof".to_owned(),
            "-nP".to_owned(),
            "-iTCP:8317".to_owned(),
            "-sTCP:LISTEN".to_owned(),
        ]]
    );
}

#[test]
fn windows_install_secures_runtime_tree_and_registers_system_startup_tasks() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths.clone());

    platform.install_services().unwrap();

    let scripts = runner.scripts();
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("SetAccessRuleProtection($true, $false)"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("FileSystemAccessRule"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("$item.GetAccessControl()"))
    );
    assert!(
        scripts
            .iter()
            .all(|script| !script.contains("Get-Acl") && !script.contains("Set-Acl"))
    );
    assert!(
        scripts
            .iter()
            .all(|script| !script.contains("Register-ScheduledTask"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("CPAStack-Block-Remote-Keeper"))
    );
    assert!(
        scripts
            .iter()
            .all(|script| !script.contains(paths.root.to_string_lossy().as_ref()))
    );
}

#[test]
fn windows_service_failure_hides_clixml_and_keeps_raw_diagnostic() {
    let temp_dir = TempDir::new().unwrap();
    let runner = RecordingRunner::with_results([CommandOutput {
        success: false,
        stdout: String::new(),
        stderr: "#< CLIXML\n<Objs><S S=\"Error\">Access is denied._x000D_</S></Objs>".into(),
    }]);
    let platform = WindowsPlatform::with_runner(runner, paths(&temp_dir));

    let error = platform.install_services().unwrap_err();

    assert_eq!(
        error.to_string(),
        "Windows 服务管理失败：访问被拒绝。请以管理员身份运行 cpactl，并执行 cpactl doctor 检查环境。"
    );
    assert_eq!(
        error.raw_diagnostic(),
        Some("#< CLIXML\n<Objs><S S=\"Error\">Access is denied._x000D_</S></Objs>")
    );
}

#[test]
fn windows_lifecycle_uses_tasks_and_disabled_marker() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths.clone());

    platform.stop(Service::Keeper).unwrap();
    platform.start(Service::Keeper).unwrap();

    assert!(!paths.disabled_file(Service::Keeper).exists());
    let scripts = runner.scripts();
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("Stop-ScheduledTask"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("Disable-ScheduledTask"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("Start-ScheduledTask"))
    );
    assert!(
        scripts
            .iter()
            .any(|script| script.contains("Enable-ScheduledTask"))
    );
}

#[test]
fn windows_activation_registers_a_cmd_task_for_the_current_release() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let release = paths.releases.join("cli-proxy-api").join("v1");
    std::fs::create_dir_all(&release).unwrap();
    std::fs::write(release.join("cli-proxy-api.exe"), "fixture").unwrap();
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths.clone());

    platform
        .replace_current_link(Service::Cli, &release)
        .unwrap();

    #[cfg(windows)]
    assert_eq!(
        std::fs::read_to_string(paths.current.join("cli-proxy-api.path"))
            .unwrap()
            .trim(),
        release.display().to_string()
    );
    let script = runner.scripts().pop().unwrap();
    assert!(script.contains("New-ScheduledTaskAction -Execute $env:ComSpec"));
    assert!(script.contains("Register-ScheduledTask"));
    assert_eq!(
        powershell_parameters(&runner.calls()[0]),
        Some(vec![
            "CPAStack-CLIProxyAPI".into(),
            release.join("cli-proxy-api.exe").display().to_string(),
            paths.config.join("config.yaml").display().to_string(),
            paths
                .logs
                .join("cli-proxy-api.out.log")
                .display()
                .to_string(),
            paths
                .logs
                .join("cli-proxy-api.err.log")
                .display()
                .to_string(),
            "-config".into(),
        ])
    );
}

#[test]
fn windows_port_health_queries_service_port_as_a_parameter() {
    let temp_dir = TempDir::new().unwrap();
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths(&temp_dir));

    assert!(platform.is_port_listening(Service::Keeper).unwrap());

    let calls = runner.calls();
    let health = calls
        .iter()
        .find(|call| {
            call.first()
                .is_some_and(|program| program == "powershell.exe")
        })
        .unwrap();
    assert_eq!(
        powershell_parameters(health),
        Some(vec!["18080".to_owned()])
    );
    assert!(health.windows(2).any(|args| {
        args[0] == "-EncodedCommand"
            && powershell_script(health)
                .is_some_and(|script| script.contains("Get-NetTCPConnection"))
    }));
}

#[test]
fn windows_status_queries_both_tasks_once_without_net_tcp_module() {
    let temp_dir = TempDir::new().unwrap();
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths(&temp_dir));

    platform.statuses().unwrap();

    let calls = runner.calls();
    let powershell_calls = calls
        .iter()
        .filter(|call| {
            call.first()
                .is_some_and(|program| program == "powershell.exe")
        })
        .collect::<Vec<_>>();
    assert_eq!(powershell_calls.len(), 1);
    let script = powershell_script(powershell_calls[0]).unwrap();
    assert!(script.contains("Get-ScheduledTask"));
    assert!(!script.contains("Get-NetTCPConnection"));
    assert_eq!(
        powershell_parameters(powershell_calls[0]),
        Some(vec![
            "CPAStack-CLIProxyAPI".to_owned(),
            "CPAStack-UsageKeeper".to_owned(),
        ])
    );
}

#[test]
fn windows_encodes_static_powershell_and_passes_runtime_values_separately() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::default();
    let platform = WindowsPlatform::with_runner(runner.clone(), paths.clone());

    platform.install_services().unwrap();

    for call in runner.calls().into_iter().filter(|call| {
        call.first()
            .is_some_and(|program| program == "powershell.exe")
    }) {
        assert!(call.iter().any(|argument| argument == "-EncodedCommand"));
        assert!(powershell_arguments(&call).is_none());
        assert!(!call.iter().any(|argument| argument == "-Command"));
        assert!(
            !call
                .iter()
                .any(|argument| argument == &paths.root.to_string_lossy())
        );
    }

    assert!(
        runner
            .scripts()
            .iter()
            .any(|script| script.contains("FromBase64String"))
    );
}
