use assert_cmd::Command;
use cpactl::app::{App, LogFollower};
use cpactl::cli::Command as CliCommand;
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
use cpactl::platform::{Platform, ServiceStatus};
use predicates::prelude::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::{Arc, Mutex};
use tempfile::TempDir;

#[test]
fn status_json_is_accepted() {
    let root = TempDir::new().unwrap().path().join("not-installed");
    Command::cargo_bin("cpactl")
        .unwrap()
        .args(["--root", root.to_str().unwrap(), "status", "--json"])
        .assert()
        .code(4)
        .stdout(predicate::str::starts_with("{\"ok\":false"));
}

#[test]
fn invalid_command_uses_usage_exit_code() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .arg("unknown")
        .assert()
        .code(2);
}

#[test]
fn path_json_is_emitted_by_the_binary() {
    let root = TempDir::new().unwrap();
    Command::cargo_bin("cpactl")
        .unwrap()
        .args(["--root", root.path().to_str().unwrap(), "--json", "path"])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"root\""));
}

#[test]
fn path_reports_the_resolved_runtime_root() {
    let fixture = Fixture::new();
    let output = fixture.app().run(&CliCommand::Path).unwrap();

    assert_eq!(
        output.data,
        json!({ "root": fixture.paths.root.display().to_string() })
    );
}

#[test]
fn status_json_does_not_expose_secret_config() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.config).unwrap();
    fs::write(
        fixture.paths.config.join("config.yaml"),
        "management-key: very-secret-value\n",
    )
    .unwrap();

    let json = fixture.app().run(&CliCommand::Status).unwrap().to_json();

    assert!(json.contains("\"services\""));
    assert!(!json.contains("very-secret-value"));
    assert!(!json.contains("management-key"));
}

#[test]
fn logs_reads_both_service_files_and_honors_line_limit() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.logs).unwrap();
    fs::write(
        fixture.paths.logs.join("cli-proxy-api.out.log"),
        "out-1\nout-2\nout-3\n",
    )
    .unwrap();
    fs::write(
        fixture.paths.logs.join("cli-proxy-api.err.log"),
        "err-1\nerr-2\nerr-3\n",
    )
    .unwrap();

    let output = fixture
        .app()
        .run(&CliCommand::Logs {
            service: "cli".into(),
            follow: false,
            lines: 2,
        })
        .unwrap();

    assert_eq!(
        output.data,
        json!({
            "service": "cli-proxy-api",
            "logs": [
                { "stream": "stdout", "lines": ["out-2", "out-3"] },
                { "stream": "stderr", "lines": ["err-2", "err-3"] }
            ]
        })
    );
}

#[test]
fn keeper_logs_use_the_canonical_service_file_prefix() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.logs).unwrap();
    fs::write(
        fixture.paths.logs.join("cpa-usage-keeper.err.log"),
        "keeper-error\n",
    )
    .unwrap();

    let output = fixture
        .app()
        .run(&CliCommand::Logs {
            service: "keeper".into(),
            follow: false,
            lines: 200,
        })
        .unwrap();

    assert_eq!(output.data["logs"][1]["lines"], json!(["keeper-error"]));
}

#[test]
fn human_status_output_includes_each_service_state() {
    let output = cpactl::output::Output::success_with_data(
        "服务状态",
        json!({
            "services": [{
                "service": "cli-proxy-api",
                "status": "运行中",
                "managed": true,
                "listening": true,
                "port": 8317,
                "version": "7.2.120"
            }]
        }),
    );

    assert_eq!(
        output.human_message(),
        "服务状态\ncli-proxy-api：运行中（端口 8317，版本 7.2.120）"
    );
}

#[test]
fn stop_writes_disabled_marker_before_platform_stop() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.root).unwrap();
    let platform = fixture.platform.clone();

    fixture
        .app()
        .run(&CliCommand::Stop {
            service: Some("cli".into()),
        })
        .unwrap();

    assert_eq!(
        platform.events(),
        vec!["stop:cli-proxy-api:disabled".to_owned()]
    );
    assert!(fixture.paths.disabled_file(Service::Cli).is_file());
}

#[test]
fn log_follower_returns_only_lines_added_since_previous_poll() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.logs).unwrap();
    let log = fixture.paths.logs.join("cli-proxy-api.out.log");
    fs::write(&log, "old\n").unwrap();
    let mut follower = LogFollower::new(vec![log]);

    assert!(follower.poll().unwrap().is_empty());
    fs::write(
        fixture.paths.logs.join("cli-proxy-api.out.log"),
        "old\nnew\n",
    )
    .unwrap();

    assert_eq!(follower.poll().unwrap(), vec!["new".to_owned()]);
}

struct Fixture {
    _temp: TempDir,
    paths: RuntimePaths,
    platform: FakePlatform,
}

impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let paths = RuntimePaths::from_root(temp.path().join("cpa-stack")).unwrap();
        let platform = FakePlatform::new(paths.clone());
        Self {
            _temp: temp,
            paths,
            platform,
        }
    }

    fn app(&self) -> App<FakePlatform> {
        App::new(self.paths.clone(), self.platform.clone())
    }
}

#[derive(Clone)]
struct FakePlatform {
    paths: RuntimePaths,
    events: Arc<Mutex<Vec<String>>>,
}

impl FakePlatform {
    fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            events: Arc::new(Mutex::new(Vec::new())),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }
}

impl Platform for FakePlatform {
    fn check_supported(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn check_permissions(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn install_services(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn remove_services(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn start(&self, _: Service) -> Result<(), AppError> {
        Ok(())
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        let marker = if self.paths.disabled_file(service).is_file() {
            "disabled"
        } else {
            "missing"
        };
        self.events
            .lock()
            .unwrap()
            .push(format!("stop:{}:{marker}", service.key()));
        Ok(())
    }

    fn restart(&self, _: Service) -> Result<(), AppError> {
        Ok(())
    }

    fn status(&self, _: Service) -> Result<ServiceStatus, AppError> {
        Ok(ServiceStatus {
            managed: true,
            disabled: false,
            listening: true,
        })
    }

    fn replace_current_link(&self, _: Service, _: &Path) -> Result<(), AppError> {
        Ok(())
    }

    fn is_port_listening(&self, _: Service) -> Result<bool, AppError> {
        Ok(true)
    }

    fn configure_firewall(&self) -> Result<(), AppError> {
        Ok(())
    }
}
