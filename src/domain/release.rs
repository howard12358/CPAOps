use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{SystemTime, UNIX_EPOCH};

use flate2::read::GzDecoder;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::storage::filesystem::RuntimeStore;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseAsset {
    pub name: String,
    #[serde(rename = "browser_download_url")]
    pub url: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReleaseMetadata {
    #[serde(rename = "tag_name")]
    pub tag: String,
    pub assets: Vec<ReleaseAsset>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ReleasePlatform {
    MacosAarch64,
    WindowsX86_64,
}

impl ReleasePlatform {
    pub fn current() -> Result<Self, AppError> {
        #[cfg(all(target_os = "macos", target_arch = "aarch64"))]
        {
            Ok(Self::MacosAarch64)
        }
        #[cfg(all(target_os = "windows", target_arch = "x86_64"))]
        {
            Ok(Self::WindowsX86_64)
        }
        #[cfg(not(any(
            all(target_os = "macos", target_arch = "aarch64"),
            all(target_os = "windows", target_arch = "x86_64")
        )))]
        {
            Err(AppError::State("当前平台或架构不受支持".into()))
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleasePlan {
    pub tag: String,
    pub asset: ReleaseAsset,
    pub checksums: ReleaseAsset,
}

impl ReleasePlan {
    pub fn from_metadata(
        service: Service,
        metadata: &ReleaseMetadata,
        platform: ReleasePlatform,
    ) -> Result<Self, AppError> {
        let matching_assets: Vec<_> = metadata
            .assets
            .iter()
            .filter(|asset| is_platform_asset(service, platform, &asset.name))
            .cloned()
            .collect();
        if matching_assets.len() != 1 {
            return Err(AppError::Verification(format!(
                "Release 必须恰有一个当前平台资产，实际找到 {} 个",
                matching_assets.len()
            )));
        }
        let checksum_assets: Vec<_> = metadata
            .assets
            .iter()
            .filter(|asset| asset.name == "checksums.txt")
            .cloned()
            .collect();
        if checksum_assets.len() != 1 {
            return Err(AppError::Verification(format!(
                "Release 必须恰有一个 checksums.txt，实际找到 {} 个",
                checksum_assets.len()
            )));
        }
        Ok(Self {
            tag: metadata.tag.clone(),
            asset: matching_assets.into_iter().next().expect("length checked"),
            checksums: checksum_assets.into_iter().next().expect("length checked"),
        })
    }
}

fn is_platform_asset(service: Service, platform: ReleasePlatform, name: &str) -> bool {
    match (service, platform) {
        (Service::Cli, ReleasePlatform::MacosAarch64) => {
            name.starts_with("CLIProxyAPI_") && name.ends_with("_darwin_aarch64.tar.gz")
        }
        (Service::Keeper, ReleasePlatform::MacosAarch64) => {
            name.starts_with("cpa-usage-keeper_v") && name.ends_with("_darwin_arm64.tar.gz")
        }
        (Service::Cli, ReleasePlatform::WindowsX86_64) => {
            name.starts_with("CLIProxyAPI_") && name.ends_with("_windows_amd64.zip")
        }
        (Service::Keeper, ReleasePlatform::WindowsX86_64) => {
            name.starts_with("cpa-usage-keeper_v") && name.ends_with("_windows_amd64.zip")
        }
    }
}

pub fn verify_checksum(archive: &Path, checksums: &Path, asset_name: &str) -> Result<(), AppError> {
    let contents = fs::read_to_string(checksums)
        .map_err(|error| AppError::Verification(format!("无法读取 checksums.txt：{error}")))?;
    let checksums: Vec<_> = contents
        .lines()
        .filter_map(parse_checksum_line)
        .filter(|(_, filename)| *filename == asset_name)
        .collect();
    if checksums.len() != 1 {
        return Err(AppError::Verification(format!(
            "校验文件中必须恰有一个 {asset_name} 的 SHA-256"
        )));
    }
    let expected = checksums[0].0;
    if expected.len() != 64 || !expected.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AppError::Verification("SHA-256 格式无效".into()));
    }
    let mut file = fs::File::open(archive)
        .map_err(|error| AppError::Verification(format!("无法读取下载资产：{error}")))?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 8192];
    loop {
        let read = file
            .read(&mut buffer)
            .map_err(|error| AppError::Verification(format!("无法计算 SHA-256：{error}")))?;
        if read == 0 {
            break;
        }
        digest.update(&buffer[..read]);
    }
    let actual = format!("{:x}", digest.finalize());
    if !actual.eq_ignore_ascii_case(expected) {
        return Err(AppError::Verification("SHA-256 校验失败".into()));
    }
    Ok(())
}

fn parse_checksum_line(line: &str) -> Option<(&str, &str)> {
    let mut parts = line.split_whitespace();
    let digest = parts.next()?;
    let filename = parts.next()?.trim_start_matches('*');
    if parts.next().is_some() {
        return None;
    }
    Some((digest, filename))
}

pub trait ServiceLifecycle {
    fn replace_current(&mut self, service: Service, release: &Path) -> Result<(), AppError>;
    fn clear_current(&mut self, service: Service) -> Result<(), AppError>;
    fn is_running(&mut self, service: Service) -> Result<bool, AppError>;
    fn start(&mut self, service: Service) -> Result<(), AppError>;
    fn stop(&mut self, service: Service) -> Result<(), AppError>;
    fn restart(&mut self, service: Service) -> Result<(), AppError>;
    fn is_healthy(&mut self, service: Service) -> Result<bool, AppError>;
    fn wait_for_healthy(&mut self, service: Service) -> Result<bool, AppError>;
}

#[derive(Clone, Debug)]
pub struct ReleaseTransaction {
    store: RuntimeStore,
}

impl ReleaseTransaction {
    pub const fn new(paths: RuntimePaths) -> Self {
        Self {
            store: RuntimeStore::new(paths),
        }
    }

    pub fn set_current(&self, service: Service, target: &Path) -> Result<(), AppError> {
        self.store.set_current(service, target)
    }

    /// 返回本机已经完成二进制验证的版本目录；未验证目录绝不能被激活或复用。
    pub fn verified_release(
        &self,
        service: Service,
        version: &str,
    ) -> Result<Option<PathBuf>, AppError> {
        let target = self.unverified_release_directory(service, version)?;
        let binary = self.binary_name(service);
        Ok((target.join(".verified").is_file() && target.join(binary).is_file()).then_some(target))
    }

    /// 只将已校验、可执行的内容写入 releases，current 的切换由 activate 负责。
    pub fn stage_verified_archive(
        &self,
        service: Service,
        version: &str,
        archive: &Path,
        checksums: &Path,
        asset_name: &str,
    ) -> Result<PathBuf, AppError> {
        verify_checksum(archive, checksums, asset_name)?;
        let destination = self.unverified_release_directory(service, version)?;
        let binary_name = self.binary_name(service);
        if destination.exists() {
            if destination.join(".verified").is_file() && destination.join(binary_name).is_file() {
                return Ok(destination);
            }
            return Err(AppError::Verification(format!(
                "版本目录已存在但未通过验证：{}",
                destination.display()
            )));
        }
        let parent = destination
            .parent()
            .ok_or_else(|| AppError::Internal("版本目录缺少父路径".into()))?;
        fs::create_dir_all(parent)
            .map_err(|error| AppError::Permission(format!("无法创建版本目录：{error}")))?;
        let temporary = self.create_temporary_release_directory(parent, version)?;
        let result = (|| {
            extract_archive(archive, &temporary)?;
            let binary = find_binary(&temporary, binary_name)?.ok_or_else(|| {
                AppError::Verification(format!("Release 缺少预期二进制：{binary_name}"))
            })?;
            let final_binary = temporary.join(binary_name);
            if binary != final_binary {
                fs::rename(&binary, &final_binary).map_err(|error| {
                    AppError::Verification(format!("无法整理 Release 二进制：{error}"))
                })?;
            }
            make_executable(&final_binary)?;
            verify_binary_starts(&final_binary)?;
            fs::write(temporary.join(".verified"), "verified\n").map_err(|error| {
                AppError::Verification(format!("无法记录 Release 验证状态：{error}"))
            })?;
            fs::rename(&temporary, &destination).map_err(|error| {
                AppError::Verification(format!("无法原子写入 Release：{error}"))
            })?;
            Ok(destination)
        })();
        if result.is_err() && temporary.exists() {
            let _ = fs::remove_dir_all(&temporary);
        }
        result
    }

    pub fn activate<L: ServiceLifecycle>(
        &self,
        service: Service,
        version: &str,
        lifecycle: &mut L,
    ) -> Result<(), AppError> {
        let target = self.release_directory(service, version)?;
        let previous_target = self.store.current_target(service)?;
        let was_running = lifecycle.is_running(service)?;

        if service == Service::Keeper {
            self.store.backup_keeper_database()?;
        }
        lifecycle.replace_current(service, &target)?;

        let activation_result = if was_running {
            lifecycle.restart(service)
        } else {
            lifecycle.start(service)
        }
        .and_then(|()| {
            if lifecycle.wait_for_healthy(service)? {
                Ok(())
            } else {
                Err(AppError::Service("新版本健康检查失败".into()))
            }
        });
        if let Err(error) = activation_result {
            return match self.rollback(service, previous_target.as_deref(), was_running, lifecycle)
            {
                Ok(()) => Err(error),
                Err(rollback_error) => Err(AppError::Service(format!(
                    "新版本激活失败且回退失败：{rollback_error}"
                ))),
            };
        }
        Ok(())
    }

    pub fn rollback<L: ServiceLifecycle>(
        &self,
        service: Service,
        previous_target: Option<&Path>,
        was_running: bool,
        lifecycle: &mut L,
    ) -> Result<(), AppError> {
        if let Some(previous_target) = previous_target {
            lifecycle.replace_current(service, previous_target)?;
        } else {
            lifecycle.clear_current(service)?;
        }

        if was_running {
            lifecycle.restart(service)
        } else {
            lifecycle.stop(service)
        }
    }

    fn release_directory(&self, service: Service, version: &str) -> Result<PathBuf, AppError> {
        self.verified_release(service, version)?
            .ok_or_else(|| AppError::State(format!("已验证版本不存在或缺少二进制：{version}")))
    }

    fn unverified_release_directory(
        &self,
        service: Service,
        version: &str,
    ) -> Result<PathBuf, AppError> {
        if version.is_empty() || version.contains('/') || version.contains('\\') {
            return Err(AppError::Usage("版本号无效".into()));
        }
        Ok(self
            .store
            .paths()
            .releases
            .join(service.key())
            .join(version))
    }

    fn binary_name(&self, service: Service) -> &'static str {
        let definition = ServiceCatalog::definition(service);
        if cfg!(target_os = "windows") {
            definition.windows_binary_name
        } else {
            definition.macos_binary_name
        }
    }

    fn create_temporary_release_directory(
        &self,
        parent: &Path,
        version: &str,
    ) -> Result<PathBuf, AppError> {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| AppError::Internal(format!("无法生成临时目录名：{error}")))?
            .as_nanos();
        for attempt in 0..100_u8 {
            let temporary = parent.join(format!(".{version}.staging-{timestamp}-{attempt}"));
            match fs::create_dir(&temporary) {
                Ok(()) => return Ok(temporary),
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(AppError::Permission(format!(
                        "无法创建 Release 临时目录：{error}"
                    )));
                }
            }
        }
        Err(AppError::Internal("无法创建唯一 Release 临时目录".into()))
    }
}

fn extract_archive(archive: &Path, destination: &Path) -> Result<(), AppError> {
    let archive_name = archive
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| AppError::Verification("Release 资产文件名无效".into()))?;
    if archive_name.ends_with(".tar.gz") {
        let file = fs::File::open(archive)
            .map_err(|error| AppError::Verification(format!("无法打开 Release 资产：{error}")))?;
        tar::Archive::new(GzDecoder::new(file))
            .unpack(destination)
            .map_err(|error| AppError::Verification(format!("无法解压 Release 资产：{error}")))?;
        return Ok(());
    }
    if archive_name.ends_with(".zip") {
        let file = fs::File::open(archive)
            .map_err(|error| AppError::Verification(format!("无法打开 Release 资产：{error}")))?;
        let mut zip = zip::ZipArchive::new(file)
            .map_err(|error| AppError::Verification(format!("无法读取 Release ZIP：{error}")))?;
        zip.extract(destination)
            .map_err(|error| AppError::Verification(format!("无法解压 Release ZIP：{error}")))?;
        return Ok(());
    }
    Err(AppError::Verification("不支持的 Release 压缩格式".into()))
}

fn find_binary(root: &Path, binary_name: &str) -> Result<Option<PathBuf>, AppError> {
    for entry in fs::read_dir(root)
        .map_err(|error| AppError::Verification(format!("无法读取解压目录：{error}")))?
    {
        let entry =
            entry.map_err(|error| AppError::Verification(format!("无法读取解压条目：{error}")))?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path)
            .map_err(|error| AppError::Verification(format!("无法读取解压条目元数据：{error}")))?;
        if metadata.file_type().is_symlink() {
            continue;
        }
        if metadata.is_file() && entry.file_name() == binary_name {
            return Ok(Some(path));
        }
        if metadata.is_dir() {
            if let Some(binary) = find_binary(&path, binary_name)? {
                return Ok(Some(binary));
            }
        }
    }
    Ok(None)
}

#[cfg(unix)]
fn make_executable(binary: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(binary, fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::Verification(format!("无法设置 Release 二进制权限：{error}")))
}

#[cfg(not(unix))]
fn make_executable(_: &Path) -> Result<(), AppError> {
    Ok(())
}

fn verify_binary_starts(binary: &Path) -> Result<(), AppError> {
    let status = Command::new(binary)
        .arg("--help")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| AppError::Verification(format!("Release 二进制无法启动：{error}")))?;
    if status.success() {
        Ok(())
    } else {
        Err(AppError::Verification("Release 二进制健康验证失败".into()))
    }
}
