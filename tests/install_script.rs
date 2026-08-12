use std::fs;

fn installer() -> String {
    fs::read_to_string("scripts/install.sh").unwrap()
}

#[test]
fn installer_supports_linux_amd64_release_assets() {
    let script = installer();

    assert!(script.contains("PLATFORM=\"linux-amd64\""));
    assert!(script.contains("ASSET=\"cpactl-$VERSION-$PLATFORM.tar.gz\""));
}

#[test]
fn linux_installer_requires_root_and_uses_global_bin_directory() {
    let script = installer();

    assert!(script.contains("id -u"));
    assert!(script.contains("/usr/local/bin"));
}

#[test]
fn installer_selects_the_native_sha256_tool() {
    let script = installer();

    assert!(script.contains("sha256sum"));
    assert!(script.contains("shasum -a 256"));
}

#[test]
fn installer_resolves_latest_release_when_version_is_not_explicit() {
    let script = installer();

    assert!(script.contains("releases/latest"));
    assert!(!script.contains("VERSION=\"${CPACTL_VERSION:-v0.1.0}\""));
}
