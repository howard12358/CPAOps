use std::fs;
use std::io::Read;

use sha2::{Digest, Sha256};
use time::OffsetDateTime;
use time::format_description::well_known::Rfc3339;

use crate::project::HOMEPAGE;

pub fn version_text() -> String {
    format!(
        "cpactl v{} ({}) {}/{}\nbuilt at: {}\n{}",
        env!("CPACTL_RELEASE_VERSION"),
        env!("CPACTL_GIT_REVISION"),
        platform_name(),
        architecture_name(),
        build_time(),
        HOMEPAGE,
    )
}

pub fn build_info_text() -> String {
    let binary_hash = std::env::current_exe()
        .ok()
        .and_then(|path| sha256_file(&path).ok())
        .unwrap_or_else(|| "无法计算".into());
    format!("{}\n二进制 SHA-256：{}", version_text(), binary_hash,)
}

fn build_time() -> String {
    env!("CPACTL_BUILD_UNIX_TIME")
        .parse::<i64>()
        .ok()
        .and_then(|timestamp| OffsetDateTime::from_unix_timestamp(timestamp).ok())
        .and_then(|timestamp| timestamp.format(&Rfc3339).ok())
        .unwrap_or_else(|| "未知".into())
}

const fn platform_name() -> &'static str {
    if cfg!(target_os = "macos") {
        "darwin"
    } else {
        std::env::consts::OS
    }
}

const fn architecture_name() -> &'static str {
    if cfg!(target_arch = "aarch64") {
        "arm64"
    } else if cfg!(target_arch = "x86_64") {
        "amd64"
    } else {
        std::env::consts::ARCH
    }
}

fn sha256_file(path: &std::path::Path) -> std::io::Result<String> {
    let mut file = fs::File::open(path)?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    Ok(format!("{:x}", digest.finalize()))
}
