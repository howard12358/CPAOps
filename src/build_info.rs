use std::fs;
use std::io::Read;

use sha2::{Digest, Sha256};

pub fn version_text() -> String {
    let binary_hash = std::env::current_exe()
        .ok()
        .and_then(|path| sha256_file(&path).ok())
        .unwrap_or_else(|| "无法计算".into());
    format!(
        "cpactl {}\nGit 提交：{}\n编译时间：{}（Unix 时间戳）\n二进制 SHA-256：{}",
        env!("CARGO_PKG_VERSION"),
        env!("CPACTL_GIT_REVISION"),
        env!("CPACTL_BUILD_UNIX_TIME"),
        binary_hash,
    )
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
