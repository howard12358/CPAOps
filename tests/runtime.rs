use std::{fs, path::PathBuf};

use cpactl::domain::{
    runtime::RuntimePaths,
    service::{Service, ServiceCatalog},
};
use cpactl::storage::filesystem::RuntimeStore;

#[test]
fn cli_aliases_resolve_to_one_catalog_entry() {
    assert_eq!(ServiceCatalog::resolve("cli").unwrap(), Service::Cli);
    assert_eq!(
        ServiceCatalog::resolve("cli-proxy-api").unwrap(),
        Service::Cli
    );
}

#[test]
fn explicit_root_beats_environment_root() {
    temp_env::with_var("CPA_STACK_ROOT", Some("/from-env"), || {
        let paths = RuntimePaths::resolve(Some(PathBuf::from("/from-cli"))).unwrap();
        assert_eq!(paths.root, PathBuf::from("/from-cli"));
    });
}

#[test]
fn cache_clean_removes_only_downloads_and_dry_run_keeps_files() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::from_root(temporary.path().join("runtime")).unwrap();
    let store = RuntimeStore::new(paths.clone());
    store.ensure_layout().unwrap();
    fs::write(paths.downloads.join("archive.tar.gz"), b"archive").unwrap();
    fs::create_dir_all(paths.downloads.join("cli").join("v1")).unwrap();
    fs::write(
        paths.downloads.join("cli").join("v1").join("checksums.txt"),
        b"sum",
    )
    .unwrap();
    fs::write(paths.config.join("config.yaml"), b"must remain").unwrap();

    let dry_run = store.clean_download_cache(true, |_| {}).unwrap();

    assert_eq!(dry_run.freed_bytes, 10);
    assert!(paths.downloads.join("archive.tar.gz").exists());

    let cleaned = store.clean_download_cache(false, |_| {}).unwrap();

    assert_eq!(cleaned.freed_bytes, 10);
    assert!(paths.downloads.is_dir());
    assert!(fs::read_dir(&paths.downloads).unwrap().next().is_none());
    assert_eq!(
        fs::read(paths.config.join("config.yaml")).unwrap(),
        b"must remain"
    );
}

#[test]
#[cfg(windows)]
fn clear_current_removes_a_windows_directory_link_without_following_its_target() {
    let temporary = tempfile::tempdir().unwrap();
    let paths = RuntimePaths::from_root(temporary.path().join("runtime")).unwrap();
    let store = RuntimeStore::new(paths.clone());
    store.ensure_layout().unwrap();
    let release = paths.releases.join("cli-proxy-api").join("v1");
    fs::create_dir_all(&release).unwrap();

    store.set_current(Service::Cli, &release).unwrap();
    store.clear_current(Service::Cli).unwrap();

    assert!(!paths.current.join(Service::Cli.key()).exists());
    assert!(release.is_dir());
}
