use std::fs;
use std::path::Path;

use crate::domain::error::AppError;
use crate::domain::release::verify_checksum;
use crate::domain::runtime::RuntimePaths;
use crate::github::GithubClient;
use crate::output::Output;
use crate::progress::ProgressReporter;
use crate::storage::config::ConfigStore;

const REPOSITORY: &str = "howard12358/CPAOps";

pub fn run(
    paths: RuntimePaths,
    check_only: bool,
    progress: &dyn ProgressReporter,
) -> Result<Output, AppError> {
    let result = run_inner(paths, check_only, progress);
    progress.clear();
    result
}

fn run_inner(
    paths: RuntimePaths,
    check_only: bool,
    progress: &dyn ProgressReporter,
) -> Result<Output, AppError> {
    progress.stage("查询 cpactl Release");
    let client = GithubClient::new(ConfigStore::new(paths))?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| AppError::Internal("无法初始化升级运行时".into()))?;
    let release = runtime.block_on(client.latest_release_for(REPOSITORY))?;
    let current = env!("CPACTL_RELEASE_VERSION");
    if release.tag.trim_start_matches('v') == current {
        return Ok(Output::success(format!("已是最新版本（{}）", release.tag)));
    }
    if check_only {
        return Ok(Output::success(format!(
            "发现新版本：{}（当前 v{current}）",
            release.tag
        )));
    }

    let asset_name = asset_name(&release.tag);
    let asset = release
        .assets
        .iter()
        .find(|asset| asset.name == asset_name)
        .ok_or_else(|| AppError::Verification("Release 缺少当前平台 cpactl 资产".into()))?;
    let checksums = release
        .assets
        .iter()
        .find(|asset| asset.name == "checksums.txt")
        .ok_or_else(|| AppError::Verification("Release 缺少 checksums.txt".into()))?;
    let temporary =
        tempfile::tempdir().map_err(|_| AppError::Permission("无法创建升级临时目录".into()))?;
    let archive = temporary.path().join(&asset.name);
    let checksum = temporary.path().join("checksums.txt");
    progress.stage("下载 cpactl 更新包");
    runtime.block_on(client.download_with_progress(&asset.url, &archive, progress))?;
    runtime.block_on(client.download_with_progress(&checksums.url, &checksum, progress))?;
    progress.stage("校验并替换 cpactl");
    verify_checksum(&archive, &checksum, &asset.name)?;
    let replacement = extract_binary(&archive, temporary.path())?;
    replace_current_binary(&replacement)?;
    Ok(Output::success(format!(
        "cpactl 已更新至 {}，请重新运行命令",
        release.tag
    )))
}

fn asset_name(tag: &str) -> String {
    asset_name_for(tag, std::env::consts::OS, std::env::consts::ARCH).unwrap_or_default()
}

fn asset_name_for(tag: &str, os: &str, arch: &str) -> Option<String> {
    match (os, arch) {
        ("macos", "aarch64") => Some(format!("cpactl-{tag}-darwin-arm64.tar.gz")),
        ("windows", "x86_64") => Some(format!("cpactl-{tag}-windows-amd64.zip")),
        ("linux", "x86_64") => Some(format!("cpactl-{tag}-linux-amd64.tar.gz")),
        _ => None,
    }
}

fn extract_binary(archive: &Path, directory: &Path) -> Result<std::path::PathBuf, AppError> {
    if archive
        .extension()
        .is_some_and(|extension| extension == "zip")
    {
        let file = fs::File::open(archive)
            .map_err(|_| AppError::Verification("无法打开 cpactl 更新包".into()))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|_| AppError::Verification("无法读取 cpactl 更新包".into()))?;
        zip.extract(directory)
            .map_err(|_| AppError::Verification("无法解压 cpactl 更新包".into()))?;
    } else {
        let file = fs::File::open(archive)
            .map_err(|_| AppError::Verification("无法打开 cpactl 更新包".into()))?;
        tar::Archive::new(flate2::read::GzDecoder::new(file))
            .unpack(directory)
            .map_err(|_| AppError::Verification("无法解压 cpactl 更新包".into()))?;
    }
    let name = if cfg!(target_os = "windows") {
        "cpactl.exe"
    } else {
        "cpactl"
    };
    let binary = directory.join(name);
    binary
        .is_file()
        .then_some(binary)
        .ok_or_else(|| AppError::Verification("cpactl 更新包缺少二进制".into()))
}

#[cfg(unix)]
fn replace_current_binary(replacement: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    let current =
        std::env::current_exe().map_err(|_| AppError::State("无法确定当前 cpactl 路径".into()))?;
    let next = current.with_extension("new");
    fs::copy(replacement, &next)
        .map_err(|_| AppError::Permission(unix_upgrade_permission_message()))?;
    fs::set_permissions(&next, fs::Permissions::from_mode(0o755))
        .map_err(|_| AppError::Permission("无法设置新版 cpactl 权限".into()))?;
    fs::rename(next, current).map_err(|_| AppError::Permission(unix_upgrade_permission_message()))
}

#[cfg(unix)]
fn unix_upgrade_permission_message() -> String {
    if cfg!(target_os = "linux") {
        "无法更新 /usr/local/bin/cpactl，请使用 sudo cpactl upgrade".into()
    } else {
        "无法写入或替换 cpactl".into()
    }
}

#[cfg(windows)]
fn replace_current_binary(replacement: &Path) -> Result<(), AppError> {
    let current =
        std::env::current_exe().map_err(|_| AppError::State("无法确定当前 cpactl 路径".into()))?;
    let next = current.with_extension("new.exe");
    fs::copy(replacement, &next).map_err(|_| AppError::Permission("无法写入新版 cpactl".into()))?;
    let script = current.with_extension("update.ps1");
    let contents = format!(
        "Wait-Process -Id {}; Move-Item -Force '{}' '{}'; Remove-Item -Force '{}'",
        std::process::id(),
        next.display(),
        current.display(),
        script.display()
    );
    fs::write(&script, contents).map_err(|_| AppError::Permission("无法创建更新脚本".into()))?;
    std::process::Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(script)
        .spawn()
        .map_err(|_| AppError::Permission("无法启动更新脚本".into()))?;
    Ok(())
}

#[cfg(not(any(unix, windows)))]
fn replace_current_binary(_: &Path) -> Result<(), AppError> {
    Err(AppError::State("当前平台不支持 cpactl 自更新".into()))
}

#[cfg(test)]
mod tests {
    #[test]
    fn asset_name_uses_the_linux_release_convention() {
        use super::asset_name_for;

        assert_eq!(
            asset_name_for("v0.1.0", "linux", "x86_64").as_deref(),
            Some("cpactl-v0.1.0-linux-amd64.tar.gz")
        );
    }

    #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
    #[test]
    fn asset_name_uses_the_macos_release_convention() {
        use super::asset_name;

        assert_eq!(asset_name("v0.1.0"), "cpactl-v0.1.0-darwin-arm64.tar.gz");
    }
}
