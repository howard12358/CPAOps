use assert_cmd::Command;
use cpactl::app::{App, LogFollower};
use cpactl::cli::Command as CliCommand;
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
use cpactl::platform::{Platform, ServiceStatus};
use cpactl::storage::{config::ConfigStore, filesystem::RuntimeStore};
use predicates::prelude::*;
use serde_json::json;
use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
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
fn error_output_exposes_raw_diagnostic_only_in_debug_mode() {
    let error = AppError::ServiceDiagnostic {
        message: "Windows 服务管理失败：访问被拒绝。请以管理员身份运行 cpactl。".into(),
        raw_diagnostic: "#< CLIXML <Objs><S S=\"Error\">Access is denied.</S></Objs>".into(),
    };

    let normal = cpactl::output::Output::from_error(&error, false);
    assert_eq!(normal.data, json!(null));
    assert!(!normal.message.contains("CLIXML"));

    let debug = cpactl::output::Output::from_error(&error, true);
    assert_eq!(
        debug.data,
        json!({
            "debug": {
                "raw_diagnostic": "#< CLIXML <Objs><S S=\"Error\">Access is denied.</S></Objs>"
            }
        })
    );
}

#[test]
fn cache_clean_json_removes_downloads_without_touching_config() {
    let root = TempDir::new().unwrap();
    let downloads = root.path().join("downloads");
    let config = root.path().join("config");
    fs::create_dir_all(&downloads).unwrap();
    fs::create_dir_all(&config).unwrap();
    fs::write(downloads.join("archive.tar.gz"), b"cache").unwrap();
    fs::write(config.join("config.yaml"), b"config").unwrap();

    Command::cargo_bin("cpactl")
        .unwrap()
        .args([
            "--root",
            root.path().to_str().unwrap(),
            "--json",
            "cache",
            "clean",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"freed_bytes\":5"));

    assert!(!downloads.join("archive.tar.gz").exists());
    assert_eq!(fs::read(config.join("config.yaml")).unwrap(), b"config");
}

#[test]
fn doctor_json_reports_checks_without_exposing_config_secrets() {
    let root = TempDir::new().unwrap();
    fs::create_dir_all(root.path().join("config")).unwrap();
    fs::write(
        root.path().join("config").join("config.yaml"),
        "management-key: very-secret-value\n",
    )
    .unwrap();

    Command::cargo_bin("cpactl")
        .unwrap()
        .args([
            "--root",
            root.path().to_str().unwrap(),
            "--json",
            "doctor",
            "--offline",
        ])
        .assert()
        .success()
        .stdout(predicate::str::contains("\"checks\""))
        .stdout(predicate::str::contains("very-secret-value").not());
}

#[test]
fn version_output_uses_compact_release_style_without_binary_hash() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::starts_with("cpactl v"))
        .stdout(predicate::str::contains("built at: "))
        .stdout(predicate::str::contains(
            "https://github.com/howard12358/CPAOps",
        ))
        .stdout(predicate::str::contains("二进制 SHA-256").not());
}

#[cfg(target_arch = "aarch64")]
#[test]
fn version_output_uses_arm64_platform_label() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .arg("-V")
        .assert()
        .success()
        .stdout(predicate::str::contains("darwin/arm64"));
}

#[test]
fn build_info_includes_binary_sha256() {
    Command::cargo_bin("cpactl")
        .unwrap()
        .arg("--build-info")
        .assert()
        .success()
        .stdout(predicate::str::contains("二进制 SHA-256："));
}

#[test]
fn help_lists_each_command_with_its_chinese_purpose() {
    let output = Command::cargo_bin("cpactl")
        .unwrap()
        .arg("-h")
        .output()
        .unwrap();
    assert!(output.status.success());
    let help = String::from_utf8(output.stdout).unwrap();
    let commands = help
        .split_once("Commands:\n")
        .and_then(|(_, rest)| rest.split_once("Options:"))
        .map(|(section, _)| section)
        .unwrap();

    assert!(commands.contains("install    安装或修复服务"));
    assert!(commands.contains("update     查询并更新到 GitHub 最新 Release"));
    assert!(commands.contains("auth       登录、查看或退出 GitHub 认证"));
    assert!(commands.contains("cache      管理可安全重新下载的缓存"));
    assert!(commands.contains("doctor     诊断本机运行环境"));
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
    let output = fixture
        .app()
        .run(&CliCommand::Path {
            open: false,
            shell: false,
        })
        .unwrap();

    assert_eq!(
        output.data,
        json!({ "root": fixture.paths.root.display().to_string() })
    );
}

#[cfg(unix)]
#[test]
fn path_shell_output_can_be_pasted_into_a_terminal() {
    let fixture = Fixture::new();
    let output = fixture
        .app()
        .run(&CliCommand::Path {
            open: false,
            shell: true,
        })
        .unwrap();

    assert_eq!(
        output.message,
        format!("cd -- '{}'", fixture.paths.root.display())
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
fn human_doctor_output_lists_check_level_and_suggestion() {
    let output = cpactl::output::Output::success_with_data(
        "诊断完成",
        json!({
            "checks": [{
                "name": "运行权限",
                "level": "fail",
                "message": "需要管理员权限",
                "suggestion": "以管理员身份重新打开 PowerShell。"
            }]
        }),
    );

    assert_eq!(
        output.human_message(),
        "诊断完成\n失败 运行权限：需要管理员权限\n  建议：以管理员身份重新打开 PowerShell。"
    );
}

#[test]
fn human_doctor_output_omits_suggestions_for_passing_checks() {
    let output = cpactl::output::Output::success_with_data(
        "诊断完成",
        json!({
            "checks": [{
                "name": "平台支持",
                "level": "pass",
                "message": "正常",
                "suggestion": "不应显示"
            }]
        }),
    );

    assert_eq!(output.human_message(), "诊断完成\n通过 平台支持：正常");
}

#[test]
fn doctor_warns_when_a_running_legacy_release_has_no_verification_marker() {
    let fixture = Fixture::new();
    let store = RuntimeStore::new(fixture.paths.clone());
    store.ensure_layout().unwrap();
    ConfigStore::new(fixture.paths.clone())
        .initialize("management-key", "keeper-password")
        .unwrap();
    for service in [Service::Cli, Service::Keeper] {
        let release = fixture.paths.releases.join(service.key()).join("legacy");
        fs::create_dir_all(&release).unwrap();
        let binary = match service {
            Service::Cli => "cli-proxy-api",
            Service::Keeper => "cpa-usage-keeper",
        };
        fs::write(release.join(binary), "legacy").unwrap();
        store.set_current(service, &release).unwrap();
    }

    let output = fixture
        .app()
        .run(&CliCommand::Doctor { offline: true })
        .unwrap();
    let checks = output.data["checks"].as_array().unwrap();
    let cli_version = checks
        .iter()
        .find(|check| check["name"] == "cli-proxy-api 当前版本")
        .unwrap();
    assert_eq!(cli_version["level"], "warning");
    assert_eq!(cli_version["message"], "旧安装版本未记录校验状态");
}

#[test]
fn human_status_output_explains_the_disabled_marker() {
    let output = cpactl::output::Output::success_with_data(
        "服务状态",
        json!({
            "services": [{
                "service": "cli-proxy-api",
                "status": "已禁用",
                "port": 8317,
                "version": "v7.2.121"
            }]
        }),
    );

    assert_eq!(
        output.human_message(),
        "服务状态\ncli-proxy-api：已停止（已禁用自动拉起）（端口 8317，版本 v7.2.121）"
    );
}

#[test]
fn human_workflow_output_includes_each_service_result() {
    let output = cpactl::output::Output::failure_with_data(
        7,
        "安装部分失败，已分别保留每项结果",
        json!({
            "services": [
                { "service": "cli-proxy-api", "ok": true },
                {
                    "service": "cpa-usage-keeper",
                    "ok": false,
                    "code": 5,
                    "message": "无法访问 GitHub，请检查网络或代理配置"
                }
            ]
        }),
    );

    assert_eq!(
        output.human_message(),
        "安装部分失败，已分别保留每项结果\ncli-proxy-api：成功\ncpa-usage-keeper：失败（无法访问 GitHub，请检查网络或代理配置）"
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
fn start_rejects_an_unregistered_service_with_state_exit_code() {
    let fixture = Fixture::new();
    fs::create_dir_all(&fixture.paths.root).unwrap();
    fixture.platform.set_managed(false);

    let error = fixture
        .app()
        .run(&CliCommand::Start {
            service: Some("cli".into()),
        })
        .unwrap_err();

    assert_eq!(error.exit_code(), 4);
    assert_eq!(error.to_string(), "服务未安装，请先运行 cpactl install");
    assert!(fixture.platform.events().is_empty());
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
    managed: Arc<AtomicBool>,
}

impl FakePlatform {
    fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            events: Arc::new(Mutex::new(Vec::new())),
            managed: Arc::new(AtomicBool::new(true)),
        }
    }

    fn events(&self) -> Vec<String> {
        self.events.lock().unwrap().clone()
    }

    fn set_managed(&self, managed: bool) {
        self.managed.store(managed, Ordering::Relaxed);
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
            managed: self.managed.load(Ordering::Relaxed),
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
