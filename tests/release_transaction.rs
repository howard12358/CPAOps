use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cpactl::domain::release::{
    ReleaseAsset, ReleaseMetadata, ReleasePlan, ReleasePlatform, ReleaseTransaction,
    ServiceLifecycle, verify_checksum,
};
use cpactl::domain::runtime::RuntimePaths;
use cpactl::domain::service::Service;
use cpactl::storage::filesystem::RuntimeStore;
use flate2::Compression;
use flate2::write::GzEncoder;
use sha2::{Digest, Sha256};
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
    let binary = match service {
        Service::Cli => "cli-proxy-api",
        Service::Keeper => "cpa-usage-keeper",
    };
    fs::write(directory.join(binary), "release binary").unwrap();
    fs::write(directory.join(".verified"), "verified\n").unwrap();
    directory
}

fn current_target(store: &RuntimeStore, service: Service) -> PathBuf {
    fs::canonicalize(store.paths().current.join(service.key())).unwrap()
}

#[derive(Default)]
struct FakeLifecycle {
    running: bool,
    healthy: bool,
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
