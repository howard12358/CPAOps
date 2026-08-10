#![cfg(windows)]

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command as ProcessCommand;
use std::sync::{Arc, Mutex};

use cpactl::app::{App, ReleaseProvider};
use cpactl::cli::Command;
use cpactl::domain::error::AppError;
use cpactl::domain::release::{ReleaseAsset, ReleaseMetadata, ReleasePlatform};
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::{Service, ServiceCatalog};
use cpactl::platform::{Platform, WindowsPlatform};
use cpactl::storage::config::ConfigStore;
use sha2::{Digest, Sha256};
use zip::write::SimpleFileOptions;

type FixtureReleases = Vec<(Service, Vec<ReleaseMetadata>)>;

#[derive(Clone)]
struct FixtureProvider {
    releases: Arc<Mutex<FixtureReleases>>,
    downloads: Arc<HashMap<String, PathBuf>>,
    version: Arc<Mutex<usize>>,
}

impl FixtureProvider {
    fn use_version(&self, version: usize) {
        *self.version.lock().unwrap() = version;
    }
}

impl ReleaseProvider for FixtureProvider {
    fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError> {
        self.releases
            .lock()
            .unwrap()
            .iter()
            .find(|(candidate, _)| *candidate == service)
            .and_then(|(_, versions)| versions.get(*self.version.lock().unwrap()))
            .cloned()
            .ok_or_else(|| AppError::Internal("缺少 Windows E2E Release fixture".into()))
    }

    fn download(&self, url: &str, destination: &Path) -> Result<PathBuf, AppError> {
        let source = self
            .downloads
            .get(url)
            .ok_or_else(|| AppError::Internal("缺少 Windows E2E 下载 fixture".into()))?;
        let parent = destination
            .parent()
            .ok_or_else(|| AppError::Internal("E2E 下载路径无父目录".into()))?;
        fs::create_dir_all(parent).map_err(|error| AppError::Permission(error.to_string()))?;
        fs::copy(source, destination).map_err(|error| AppError::Permission(error.to_string()))?;
        Ok(destination.to_path_buf())
    }
}

#[test]
fn windows_install_update_rollback_and_uninstall_use_real_tasks_and_fixture_releases() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::from_root(temporary.path().join("runtime")).unwrap();
    let provider = fixture_provider(temporary.path());
    let config = ConfigStore::new(paths.clone());
    config
        .initialize("e2e-management-key", "e2e-keeper-password")
        .unwrap();
    let app = App::with_release_provider(
        paths.clone(),
        WindowsPlatform::new(paths.clone()),
        provider.clone(),
        ReleasePlatform::WindowsX86_64,
    );

    let installation = app.run(&Command::Install).unwrap();
    assert!(
        installation.data["services"]
            .as_array()
            .is_some_and(|services| {
                services
                    .iter()
                    .all(|service| service["ok"].as_bool() == Some(true))
            }),
        "安装结果：{}\n服务日志：{}\n任务诊断：{}",
        installation.to_json(),
        service_logs(&paths),
        task_diagnostics(&paths),
    );
    assert!(
        WindowsPlatform::new(paths.clone())
            .status(Service::Cli)
            .unwrap()
            .listening
    );
    assert!(
        WindowsPlatform::new(paths.clone())
            .status(Service::Keeper)
            .unwrap()
            .listening
    );

    provider.use_version(1);
    app.run(&Command::Update { service: None }).unwrap();
    assert_current_version(&paths, Service::Cli, "v2");
    assert_current_version(&paths, Service::Keeper, "v2");

    app.run(&Command::Rollback {
        service: "cli".into(),
        version: "v1".into(),
    })
    .unwrap();
    assert_current_version(&paths, Service::Cli, "v1");

    app.run(&Command::Uninstall { purge: false }).unwrap();
    assert!(
        !WindowsPlatform::new(paths)
            .status(Service::Cli)
            .unwrap()
            .managed
    );
}

fn fixture_provider(directory: &Path) -> FixtureProvider {
    let fixture = PathBuf::from(env!("CARGO_BIN_EXE_cpactl-test-service"));
    let mut releases = Vec::new();
    let mut downloads = HashMap::new();
    for service in [Service::Cli, Service::Keeper] {
        let mut versions = Vec::new();
        for version in ["v1", "v2"] {
            let asset_name = asset_name(service, version);
            let archive = directory.join(format!("{service:?}-{version}.zip"));
            create_archive(
                &archive,
                &fixture,
                ServiceCatalog::definition(service).windows_binary_name,
            );
            let checksums = directory.join(format!("{service:?}-{version}.checksums.txt"));
            fs::write(&checksums, format!("{}  {asset_name}\n", sha256(&archive))).unwrap();
            let asset_url = format!("fixture://{service:?}/{version}/asset");
            let checksum_url = format!("fixture://{service:?}/{version}/checksums");
            downloads.insert(asset_url.clone(), archive);
            downloads.insert(checksum_url.clone(), checksums);
            versions.push(ReleaseMetadata {
                tag: version.into(),
                assets: vec![
                    ReleaseAsset {
                        name: asset_name,
                        url: asset_url,
                    },
                    ReleaseAsset {
                        name: "checksums.txt".into(),
                        url: checksum_url,
                    },
                ],
            });
        }
        releases.push((service, versions));
    }
    FixtureProvider {
        releases: Arc::new(Mutex::new(releases)),
        downloads: Arc::new(downloads),
        version: Arc::new(Mutex::new(0)),
    }
}

fn asset_name(service: Service, version: &str) -> String {
    match service {
        Service::Cli => format!("CLIProxyAPI_{version}_windows_amd64.zip"),
        Service::Keeper => format!("cpa-usage-keeper_{version}_windows_amd64.zip"),
    }
}

fn create_archive(path: &Path, fixture: &Path, name: &str) {
    let mut archive = zip::ZipWriter::new(File::create(path).unwrap());
    archive
        .start_file(name, SimpleFileOptions::default())
        .unwrap();
    archive.write_all(&fs::read(fixture).unwrap()).unwrap();
    archive.finish().unwrap();
}

fn sha256(path: &Path) -> String {
    format!("{:x}", Sha256::digest(fs::read(path).unwrap()))
}

fn assert_current_version(paths: &RuntimePaths, service: Service, expected: &str) {
    let target = cpactl::storage::filesystem::RuntimeStore::new(paths.clone())
        .current_target(service)
        .unwrap()
        .unwrap();
    assert_eq!(target.file_name().unwrap(), expected);
}

fn service_logs(paths: &RuntimePaths) -> String {
    ["cli-proxy-api", "cpa-usage-keeper"]
        .into_iter()
        .flat_map(|service| ["out", "err"].map(move |stream| (service, stream)))
        .map(|(service, stream)| {
            let path = paths.logs.join(format!("{service}.{stream}.log"));
            format!(
                "{}={}",
                path.display(),
                fs::read_to_string(&path).unwrap_or_default()
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn task_diagnostics(paths: &RuntimePaths) -> String {
    let task_state = ProcessCommand::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            concat!(
                "$ErrorActionPreference = 'Continue'; ",
                "$tasks = 'CPAStack-CLIProxyAPI', 'CPAStack-UsageKeeper'; ",
                "foreach ($task in $tasks) { ",
                "Write-Output \"=== $task ===\"; ",
                "Get-ScheduledTask -TaskName $task | Format-List TaskName,State,Actions,Principal; ",
                "Get-ScheduledTaskInfo -TaskName $task | Format-List LastRunTime,LastTaskResult,NumberOfMissedRuns,NextRunTime; ",
                "Export-ScheduledTask -TaskName $task; ",
                "}"
            ),
        ])
        .output()
        .map(|output| {
            format!(
                "{}{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            )
        })
        .unwrap_or_else(|error| format!("无法读取任务诊断：{error}"));
    let files = [
        paths.tasks.join("run-cli-proxy-api.ps1"),
        paths.tasks.join("run-cpa-usage-keeper.ps1"),
        paths.current.join("cli-proxy-api.path"),
        paths.current.join("cpa-usage-keeper.path"),
    ]
    .into_iter()
    .map(|path| {
        format!(
            "{}={}",
            path.display(),
            fs::read_to_string(&path).unwrap_or_else(|error| format!("<无法读取：{error}>"))
        )
    })
    .collect::<Vec<_>>()
    .join("\n");
    format!("{task_state}\n{files}")
}
