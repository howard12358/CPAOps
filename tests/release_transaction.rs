use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

use cpactl::domain::release::{
    ReleaseAsset, ReleaseMetadata, ReleasePlan, ReleasePlatform, ReleaseTransaction,
    ServiceLifecycle, verify_checksum,
};
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::{Service, ServiceCatalog};
use cpactl::storage::{config::ConfigStore, filesystem::RuntimeStore};
use cpactl::{
    app::{App, ReleaseProvider},
    cli::Command,
    domain::error::AppError,
    platform::{Platform, ServiceStatus},
};
#[cfg(unix)]
use flate2::{Compression, write::GzEncoder};
#[cfg(unix)]
use sha2::{Digest, Sha256};
#[cfg(unix)]
use tar::Builder;

static TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn test_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("cpactl-release-test-{nonce}-{counter}"))
}

fn runtime() -> (PathBuf, RuntimeStore) {
    let root = test_root();
    let paths = RuntimePaths::from_root(root.clone()).unwrap();
    let store = RuntimeStore::new(paths);
    store.ensure_layout().unwrap();
    (root, store)
}

fn metadata(assets: Vec<ReleaseAsset>) -> ReleaseMetadata {
    ReleaseMetadata {
        tag: "v1.2.3".into(),
        assets,
    }
}

fn asset(name: &str) -> ReleaseAsset {
    ReleaseAsset {
        name: name.into(),
        url: format!("https://example.invalid/{name}"),
    }
}

fn release_dir(store: &RuntimeStore, service: Service, version: &str) -> PathBuf {
    store.paths().releases.join(service.key()).join(version)
}

fn create_release(store: &RuntimeStore, service: Service, version: &str) -> PathBuf {
    let directory = release_dir(store, service, version);
    fs::create_dir_all(&directory).unwrap();
    let definition = ServiceCatalog::definition(service);
    let binary = if cfg!(target_os = "windows") {
        definition.windows_binary_name
    } else {
        definition.macos_binary_name
    };
    fs::write(directory.join(binary), "release binary").unwrap();
    fs::write(directory.join(".verified"), "verified\n").unwrap();
    directory
}

fn current_target(store: &RuntimeStore, service: Service) -> PathBuf {
    store
        .current_target(service)
        .unwrap()
        .unwrap()
        .canonicalize()
        .unwrap()
}

struct FakeLifecycle {
    running: bool,
    healthy: bool,
    paths: RuntimePaths,
}

impl ServiceLifecycle for FakeLifecycle {
    fn is_running(&mut self, _: Service) -> Result<bool, cpactl::domain::error::AppError> {
        Ok(self.running)
    }

    fn start(&mut self, _: Service) -> Result<(), cpactl::domain::error::AppError> {
        self.running = true;
        Ok(())
    }

    fn stop(&mut self, _: Service) -> Result<(), cpactl::domain::error::AppError> {
        self.running = false;
        Ok(())
    }

    fn restart(&mut self, _: Service) -> Result<(), cpactl::domain::error::AppError> {
        self.running = true;
        Ok(())
    }

    fn is_healthy(&mut self, _: Service) -> Result<bool, cpactl::domain::error::AppError> {
        Ok(self.healthy)
    }

    fn wait_for_healthy(
        &mut self,
        service: Service,
    ) -> Result<bool, cpactl::domain::error::AppError> {
        self.is_healthy(service)
    }

    fn replace_current(
        &mut self,
        service: Service,
        release: &Path,
    ) -> Result<(), cpactl::domain::error::AppError> {
        RuntimeStore::new(self.paths.clone()).set_current(service, release)
    }

    fn clear_current(&mut self, service: Service) -> Result<(), cpactl::domain::error::AppError> {
        RuntimeStore::new(self.paths.clone()).clear_current(service)
    }
}

#[test]
fn chooses_only_the_current_platform_asset() {
    let metadata = metadata(vec![
        asset("CLIProxyAPI_1.2.3_darwin_aarch64.tar.gz"),
        asset("CLIProxyAPI_1.2.3_windows_amd64.zip"),
        asset("checksums.txt"),
    ]);

    let plan =
        ReleasePlan::from_metadata(Service::Cli, &metadata, ReleasePlatform::MacosAarch64).unwrap();

    assert_eq!(
        plan.asset.url,
        "https://example.invalid/CLIProxyAPI_1.2.3_darwin_aarch64.tar.gz"
    );
    assert_eq!(plan.checksums.url, "https://example.invalid/checksums.txt");
}

#[test]
fn checksum_failure_leaves_current_target_unchanged() {
    let (root, store) = runtime();
    let old = create_release(&store, Service::Cli, "v1");
    let transaction = ReleaseTransaction::new(store.paths().clone());
    transaction.set_current(Service::Cli, &old).unwrap();
    let archive = root.join("bad.tar.gz");
    let checksums = root.join("checksums.txt");
    fs::write(&archive, "not the expected archive").unwrap();
    fs::write(
        &checksums,
        "0000000000000000000000000000000000000000000000000000000000000000  bad.tar.gz\n",
    )
    .unwrap();

    assert!(verify_checksum(&archive, &checksums, "bad.tar.gz").is_err());
    assert_eq!(
        current_target(&store, Service::Cli),
        old.canonicalize().unwrap()
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_health_check_restores_previous_release_and_running_state() {
    let (root, store) = runtime();
    let old = create_release(&store, Service::Cli, "v1");
    create_release(&store, Service::Cli, "v2");
    let transaction = ReleaseTransaction::new(store.paths().clone());
    transaction.set_current(Service::Cli, &old).unwrap();
    let mut lifecycle = FakeLifecycle {
        running: true,
        healthy: false,
        paths: store.paths().clone(),
    };

    assert!(
        transaction
            .activate(Service::Cli, "v2", &mut lifecycle)
            .is_err()
    );
    assert_eq!(
        current_target(&store, Service::Cli),
        old.canonicalize().unwrap()
    );
    assert!(lifecycle.running);

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn activating_keeper_copies_database_wal_and_shm_to_one_backup() {
    let (root, store) = runtime();
    create_release(&store, Service::Keeper, "v1");
    fs::write(store.paths().keeper.join("app.db"), "database").unwrap();
    fs::write(store.paths().keeper.join("app.db-wal"), "wal").unwrap();
    fs::write(store.paths().keeper.join("app.db-shm"), "shm").unwrap();
    let transaction = ReleaseTransaction::new(store.paths().clone());
    let mut lifecycle = FakeLifecycle {
        running: false,
        healthy: true,
        paths: store.paths().clone(),
    };

    transaction
        .activate(Service::Keeper, "v1", &mut lifecycle)
        .unwrap();

    let backups = fs::read_dir(store.paths().keeper.join("migration-backups"))
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(backups.len(), 1);
    let backup = backups[0].path();
    assert_eq!(
        fs::read_to_string(backup.join("app.db")).unwrap(),
        "database"
    );
    assert_eq!(
        fs::read_to_string(backup.join("app.db-wal")).unwrap(),
        "wal"
    );
    assert_eq!(
        fs::read_to_string(backup.join("app.db-shm")).unwrap(),
        "shm"
    );

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn verified_archive_is_staged_before_its_release_becomes_available() {
    use std::os::unix::fs::PermissionsExt;

    let (root, store) = runtime();
    let archive = root.join("CLIProxyAPI_1.2.3_darwin_aarch64.tar.gz");
    let archive_file = fs::File::create(&archive).unwrap();
    let encoder = GzEncoder::new(archive_file, Compression::default());
    let mut tar = Builder::new(encoder);
    let binary = b"#!/bin/sh\nexit 0\n";
    let mut header = tar::Header::new_gnu();
    header.set_size(binary.len() as u64);
    header.set_mode(0o700);
    header.set_cksum();
    tar.append_data(&mut header, "bundle/cli-proxy-api", &binary[..])
        .unwrap();
    tar.finish().unwrap();
    let encoder = tar.into_inner().unwrap();
    encoder.finish().unwrap();
    fs::set_permissions(&archive, fs::Permissions::from_mode(0o600)).unwrap();
    let digest = format!("{:x}", Sha256::digest(fs::read(&archive).unwrap()));
    let checksums = root.join("checksums.txt");
    fs::write(
        &checksums,
        format!("{digest}  CLIProxyAPI_1.2.3_darwin_aarch64.tar.gz\n"),
    )
    .unwrap();
    let transaction = ReleaseTransaction::new(store.paths().clone());

    let staged = transaction
        .stage_verified_archive(
            Service::Cli,
            "v1.2.3",
            &archive,
            &checksums,
            "CLIProxyAPI_1.2.3_darwin_aarch64.tar.gz",
        )
        .unwrap();

    assert_eq!(staged, release_dir(&store, Service::Cli, "v1.2.3"));
    assert!(staged.join("cli-proxy-api").is_file());
    assert!(staged.join(".verified").is_file());
    assert!(!store.paths().current.join(Service::Cli.key()).exists());

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn failed_keeper_update_restores_keeper_but_keeps_successful_cli_update() {
    let (root, store) = runtime();
    let cli_v1 = create_release(&store, Service::Cli, "v1");
    let keeper_v1 = create_release(&store, Service::Keeper, "v1");
    let _cli_v2 = create_release(&store, Service::Cli, "v2");
    let keeper_v2 = create_release(&store, Service::Keeper, "v2");
    let transaction = ReleaseTransaction::new(store.paths().clone());
    transaction.set_current(Service::Cli, &cli_v1).unwrap();
    transaction
        .set_current(Service::Keeper, &keeper_v1)
        .unwrap();
    ConfigStore::new(store.paths().clone())
        .initialize("management-key", "keeper-password")
        .unwrap();

    let platform = UpdatePlatform::new(store.paths().clone());
    let app = App::with_release_provider(
        store.paths().clone(),
        platform.clone(),
        StaticReleaseProvider::new([(Service::Cli, "v2"), (Service::Keeper, "v2")]),
        ReleasePlatform::MacosAarch64,
    );

    let output = app.run(&Command::Update { service: None }).unwrap();

    assert!(!output.ok);
    assert_eq!(output.data["services"][0]["ok"], true);
    assert_eq!(
        platform
            .current_target(Service::Cli)
            .and_then(|path| path.file_name().map(|name| name.to_owned())),
        Some("v2".into())
    );
    assert_eq!(
        platform
            .current_target(Service::Keeper)
            .and_then(|path| path.file_name().map(|name| name.to_owned())),
        Some("v1".into())
    );
    assert!(keeper_v2.is_dir());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn update_reports_when_a_service_is_already_at_the_latest_version() {
    let (root, store) = runtime();
    let current = create_release(&store, Service::Cli, "v1");
    ReleaseTransaction::new(store.paths().clone())
        .set_current(Service::Cli, &current)
        .unwrap();
    ConfigStore::new(store.paths().clone())
        .initialize("management-key", "keeper-password")
        .unwrap();
    let platform = UpdatePlatform::new(store.paths().clone());
    let app = App::with_release_provider(
        store.paths().clone(),
        platform,
        StaticReleaseProvider::new([(Service::Cli, "v1")]),
        ReleasePlatform::MacosAarch64,
    );

    let output = app
        .run(&Command::Update {
            service: Some("cli".into()),
        })
        .unwrap();

    assert_eq!(
        output.data["services"][0],
        serde_json::json!({
            "service": "cli-proxy-api",
            "ok": true,
            "version": "v1",
            "state": "up_to_date"
        })
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn rollback_rejects_version_not_present_in_verified_releases() {
    let (root, store) = runtime();
    let version = release_dir(&store, Service::Cli, "v2");
    fs::create_dir_all(&version).unwrap();
    fs::write(version.join("cli-proxy-api"), "unverified").unwrap();
    let app = App::with_release_provider(
        store.paths().clone(),
        UpdatePlatform::new(store.paths().clone()),
        StaticReleaseProvider::new([]),
        ReleasePlatform::MacosAarch64,
    );

    let error = app
        .run(&Command::Rollback {
            service: "cli".into(),
            version: "v2".into(),
        })
        .unwrap_err();

    assert!(matches!(error, AppError::State(_)));
    fs::remove_dir_all(root).unwrap();
}

struct StaticReleaseProvider {
    versions: Vec<(Service, String)>,
}

impl StaticReleaseProvider {
    fn new<const N: usize>(versions: [(Service, &str); N]) -> Self {
        Self {
            versions: versions
                .into_iter()
                .map(|(service, version)| (service, version.into()))
                .collect(),
        }
    }
}

impl ReleaseProvider for StaticReleaseProvider {
    fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError> {
        let version = self
            .versions
            .iter()
            .find_map(|(candidate, version)| (*candidate == service).then_some(version))
            .ok_or_else(|| AppError::Network("测试 Release 不存在".into()))?;
        let asset = match service {
            Service::Cli => format!("CLIProxyAPI_{version}_darwin_aarch64.tar.gz"),
            Service::Keeper => format!("cpa-usage-keeper_{version}_darwin_arm64.tar.gz"),
        };
        Ok(ReleaseMetadata {
            tag: version.clone(),
            assets: vec![
                ReleaseAsset {
                    name: asset.clone(),
                    url: format!("memory://{asset}"),
                },
                ReleaseAsset {
                    name: "checksums.txt".into(),
                    url: "memory://checksums.txt".into(),
                },
            ],
        })
    }

    fn download(&self, _: &str, _: &Path) -> Result<PathBuf, AppError> {
        Err(AppError::Internal("测试不应下载已验证版本".into()))
    }
}

#[derive(Clone)]
struct UpdatePlatform {
    paths: RuntimePaths,
    current: Arc<Mutex<Vec<(Service, PathBuf)>>>,
}

impl UpdatePlatform {
    fn new(paths: RuntimePaths) -> Self {
        Self {
            paths,
            current: Arc::default(),
        }
    }

    fn current_target(&self, service: Service) -> Option<PathBuf> {
        self.current
            .lock()
            .unwrap()
            .iter()
            .find_map(|(candidate, path)| (*candidate == service).then(|| path.clone()))
    }
}

impl Platform for UpdatePlatform {
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
    fn stop(&self, _: Service) -> Result<(), AppError> {
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
    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        let mut current = self.current.lock().unwrap();
        if let Some((_, path)) = current
            .iter_mut()
            .find(|(candidate, _)| *candidate == service)
        {
            *path = release.to_path_buf();
        } else {
            current.push((service, release.to_path_buf()));
        }
        Ok(())
    }
    fn is_port_listening(&self, service: Service) -> Result<bool, AppError> {
        Ok(service == Service::Cli || !self.paths.current.join(Service::Keeper.key()).exists())
    }
    fn wait_for_port(&self, service: Service) -> Result<bool, AppError> {
        self.is_port_listening(service)
    }
    fn configure_firewall(&self) -> Result<(), AppError> {
        Ok(())
    }
}
