use std::ffi::OsString;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
use cpactl::platform::{CommandOutput, CommandRunner, MacosPlatform, Platform};
use tempfile::TempDir;

#[derive(Clone, Default)]
struct RecordingRunner {
    calls: Arc<Mutex<Vec<Vec<String>>>>,
}

impl RecordingRunner {
    fn calls(&self) -> Vec<Vec<String>> {
        self.calls.lock().unwrap().clone()
    }
}

impl CommandRunner for RecordingRunner {
    fn run(&self, program: &str, args: &[OsString]) -> Result<CommandOutput, AppError> {
        let mut call = vec![program.to_owned()];
        call.extend(args.iter().map(|arg| arg.to_string_lossy().into_owned()));
        self.calls.lock().unwrap().push(call);
        Ok(CommandOutput::success())
    }
}

fn paths(temp_dir: &TempDir) -> RuntimePaths {
    RuntimePaths::from_root(PathBuf::from(temp_dir.path())).unwrap()
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
            "gui/501/io.cpa-local.cli-proxy-api".to_owned(),
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
            "gui/501/io.cpa-local.usage-keeper".to_owned(),
        ]]
    );
}

#[test]
fn macos_install_writes_validated_plists_and_service_wrappers() {
    let temp_dir = TempDir::new().unwrap();
    let paths = paths(&temp_dir);
    let runner = RecordingRunner::default();
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
                "bootstrap".to_owned(),
                "gui/501".to_owned(),
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
                "bootstrap".to_owned(),
                "gui/501".to_owned(),
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
