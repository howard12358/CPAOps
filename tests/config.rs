use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use cpactl::domain::runtime::RuntimePaths;
use cpactl::storage::config::{ConfigStore, GithubTokenStore, ProxyConfig, Redacted};
use cpactl::storage::filesystem::RuntimeStore;

static TEST_ROOT_COUNTER: AtomicU64 = AtomicU64::new(0);

fn test_root() -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let counter = TEST_ROOT_COUNTER.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("cpactl-config-test-{nonce}-{counter}"))
}

fn store() -> (PathBuf, RuntimeStore, ConfigStore) {
    let root = test_root();
    let paths = RuntimePaths::from_root(root.clone()).unwrap();
    let runtime = RuntimeStore::new(paths);
    let config = ConfigStore::new(runtime.paths().clone());
    (root, runtime, config)
}

fn write_private_file(path: &std::path::Path, contents: &str) {
    fs::write(path, contents).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    }
}

#[test]
fn layout_creates_all_runtime_directories() {
    let (root, runtime, _) = store();

    runtime.ensure_layout().unwrap();

    for path in [
        &runtime.paths().config,
        &runtime.paths().auths,
        &runtime.paths().keeper,
        &runtime.paths().releases,
        &runtime.paths().current,
        &runtime.paths().downloads,
        &runtime.paths().logs,
        &runtime.paths().state,
        &runtime.paths().bin,
        &runtime.paths().tasks,
    ] {
        assert!(
            path.is_dir(),
            "missing runtime directory: {}",
            path.display()
        );
    }

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn initialization_keeps_existing_config() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    let cpa_config = runtime.paths().config.join("config.yaml");
    fs::write(&cpa_config, "kept\n").unwrap();

    config
        .initialize("management-key", "login-password")
        .unwrap();

    assert_eq!(fs::read_to_string(cpa_config).unwrap(), "kept\n");
    assert!(runtime.paths().config.join("keeper.env").is_file());
    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn initialization_secures_existing_config_without_replacing_it() {
    use std::os::unix::fs::PermissionsExt;

    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    let cpa_config = runtime.paths().config.join("config.yaml");
    fs::write(&cpa_config, "kept\n").unwrap();
    fs::set_permissions(&cpa_config, fs::Permissions::from_mode(0o644)).unwrap();

    config
        .initialize("management-key", "login-password")
        .unwrap();

    assert_eq!(fs::read_to_string(&cpa_config).unwrap(), "kept\n");
    assert_eq!(
        fs::metadata(cpa_config).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validation_rejects_required_placeholder() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    write_private_file(
        &runtime.paths().config.join("config.yaml"),
        "remote-management:\n  secret-key: __REQUIRED__\n",
    );
    write_private_file(
        &runtime.paths().config.join("keeper.env"),
        "CPA_MANAGEMENT_KEY=__REQUIRED__\n",
    );

    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("占位符")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validation_rejects_cpa_port_outside_the_allowed_range() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    write_private_file(
        &runtime.paths().config.join("config.yaml"),
        "port: 0\nremote-management:\n  secret-key: configured\n",
    );
    write_private_file(
        &runtime.paths().config.join("keeper.env"),
        "CPA_MANAGEMENT_KEY=configured\nAPP_PORT=18080\n",
    );

    assert!(config.validate().unwrap_err().to_string().contains("port"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validation_rejects_keeper_port_outside_the_allowed_range() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    write_private_file(
        &runtime.paths().config.join("config.yaml"),
        "port: 8317\nremote-management:\n  secret-key: configured\n",
    );
    write_private_file(
        &runtime.paths().config.join("keeper.env"),
        "CPA_MANAGEMENT_KEY=configured\nAPP_PORT=65536\n",
    );

    assert!(
        config
            .validate()
            .unwrap_err()
            .to_string()
            .contains("APP_PORT")
    );

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn validation_rejects_duplicate_keeper_app_port() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    write_private_file(
        &runtime.paths().config.join("config.yaml"),
        "port: 8317\nremote-management:\n  secret-key: configured\n",
    );
    write_private_file(
        &runtime.paths().config.join("keeper.env"),
        "CPA_MANAGEMENT_KEY=configured\nAPP_PORT=18080\nAPP_PORT=18081\n",
    );

    assert!(config.validate().unwrap_err().to_string().contains("重复"));

    fs::remove_dir_all(root).unwrap();
}

#[test]
fn proxy_display_never_contains_its_url() {
    let proxy = ProxyConfig::parse("https_proxy=http://user:secret@host:7890").unwrap();
    let summary = proxy.redacted_summary();

    assert!(!summary.contains("secret"));
    assert!(!summary.contains("host"));
    assert!(!summary.contains("http://"));
}

#[test]
fn proxy_debug_never_contains_its_url() {
    let proxy = ProxyConfig::parse("https_proxy=http://user:secret@host:7890").unwrap();

    let debug_output = format!("{proxy:?}");
    assert!(!debug_output.contains("secret"));
    assert!(!debug_output.contains("host"));
}

#[test]
fn proxy_store_round_trip_does_not_require_exposing_url() {
    let (root, runtime, config) = store();
    runtime.ensure_layout().unwrap();
    let proxy = ProxyConfig::parse("all_proxy=socks5://user:secret@host:1080").unwrap();

    config.save_proxy(&proxy).unwrap();

    let loaded = config.load_proxy().unwrap().unwrap();
    assert_eq!(loaded.redacted_summary(), "已配置代理");
    config.clear_proxy().unwrap();
    assert!(config.load_proxy().unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn proxy_from_url_uses_the_url_for_all_protocols() {
    let proxy = ProxyConfig::from_url("socks5://127.0.0.1:7890").unwrap();

    assert_eq!(proxy.redacted_summary(), "已配置代理");
}

#[test]
fn proxy_from_url_accepts_exported_environment_assignments() {
    let proxy = ProxyConfig::from_url(
        "export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897 all_proxy=socks5://127.0.0.1:7897",
    )
    .unwrap();

    assert_eq!(proxy.redacted_summary(), "已配置代理");
}

#[test]
fn proxy_rejects_unknown_key_and_unsupported_scheme() {
    assert!(ProxyConfig::parse("ftp_proxy=http://host:7890").is_err());
    assert!(ProxyConfig::parse("https_proxy=ftp://host:7890").is_err());
}

#[test]
fn token_status_and_redacted_display_do_not_reveal_secret() {
    let (root, runtime, _) = store();
    runtime.ensure_layout().unwrap();
    let token_store = GithubTokenStore::at(root.join("github-token"));
    token_store.save("token-value").unwrap();

    assert!(token_store.load().unwrap().is_some());
    assert_eq!(Redacted::new("token-value").to_string(), "已配置");

    fs::remove_dir_all(root).unwrap();
}

#[cfg(unix)]
#[test]
fn saving_token_creates_a_private_file() {
    use std::os::unix::fs::PermissionsExt;

    let (root, runtime, _) = store();
    runtime.ensure_layout().unwrap();
    let token_store = GithubTokenStore::at(root.join("github-token"));

    token_store.save("token-value").unwrap();

    let token_path = token_store.path();
    assert!(token_store.load().unwrap().is_some());
    assert_eq!(
        fs::metadata(token_path).unwrap().permissions().mode() & 0o777,
        0o600
    );
    fs::remove_dir_all(root).unwrap();
}

#[test]
fn clearing_token_removes_saved_authentication() {
    let (root, runtime, _) = store();
    runtime.ensure_layout().unwrap();
    let token_store = GithubTokenStore::at(root.join("github-token"));
    token_store.save("token-value").unwrap();

    token_store.clear().unwrap();

    assert!(token_store.load().unwrap().is_none());
    fs::remove_dir_all(root).unwrap();
}
