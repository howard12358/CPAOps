use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::Service;

#[derive(Clone, Debug)]
pub struct RuntimeStore {
    paths: RuntimePaths,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct CacheCleanup {
    pub removed_entries: u64,
    pub freed_bytes: u64,
}

impl RuntimeStore {
    pub const fn new(paths: RuntimePaths) -> Self {
        Self { paths }
    }

    pub const fn paths(&self) -> &RuntimePaths {
        &self.paths
    }

    pub fn ensure_layout(&self) -> Result<(), AppError> {
        for directory in [
            &self.paths.root,
            &self.paths.config,
            &self.paths.auths,
            &self.paths.keeper,
            &self.paths.releases,
            &self.paths.current,
            &self.paths.downloads,
            &self.paths.logs,
            &self.paths.state,
            &self.paths.bin,
            &self.paths.tasks,
        ] {
            fs::create_dir_all(directory)
                .map_err(|error| AppError::Permission(format!("无法创建运行目录：{error}")))?;
            set_private_directory(directory)?;
        }
        Ok(())
    }

    /// 原子更新 current 链接，避免服务管理器观察到半写入的目标。
    pub fn set_current(&self, service: Service, target: &Path) -> Result<(), AppError> {
        if !target.is_dir() {
            return Err(AppError::State(format!(
                "待激活版本目录不存在：{}",
                target.display()
            )));
        }

        fs::create_dir_all(&self.paths.current)
            .map_err(|error| AppError::Permission(format!("无法创建 current 目录：{error}")))?;
        #[cfg(windows)]
        {
            self.set_windows_current_pointer(service, target)
        }
        #[cfg(not(windows))]
        {
            let current = self.paths.current.join(service.key());
            let next = self.paths.current.join(format!("{}.next", service.key()));
            remove_path_if_exists(&next)?;
            create_directory_link(target, &next)?;
            replace_current_link(&next, &current)
        }
    }

    pub fn clear_current(&self, service: Service) -> Result<(), AppError> {
        #[cfg(windows)]
        {
            remove_path_if_exists(&self.windows_current_pointer(service))
        }
        #[cfg(not(windows))]
        {
            remove_path_if_exists(&self.paths.current.join(service.key()))
        }
    }

    pub fn clean_download_cache<F>(
        &self,
        dry_run: bool,
        mut on_progress: F,
    ) -> Result<CacheCleanup, AppError>
    where
        F: FnMut(CacheCleanup),
    {
        let downloads = &self.paths.downloads;
        let summary = match fs::symlink_metadata(downloads) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(AppError::Usage("拒绝清理符号链接形式的下载缓存目录".into()));
            }
            Ok(metadata) if metadata.is_dir() => inspect_cache_directory(downloads)?,
            Ok(_) => return Err(AppError::Usage("下载缓存路径不是目录".into())),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => CacheCleanup::default(),
            Err(error) => {
                return Err(AppError::Permission(format!(
                    "无法读取下载缓存目录：{error}"
                )));
            }
        };
        if dry_run || summary.removed_entries == 0 {
            return Ok(summary);
        }

        let mut removed = CacheCleanup::default();
        for entry in fs::read_dir(downloads)
            .map_err(|error| AppError::Permission(format!("无法读取下载缓存目录：{error}")))?
        {
            let path = entry
                .map_err(|error| AppError::Permission(format!("无法读取下载缓存内容：{error}")))?
                .path();
            remove_cache_path(&path, &mut removed, &mut on_progress)?;
        }
        Ok(removed)
    }

    pub fn current_target(&self, service: Service) -> Result<Option<PathBuf>, AppError> {
        #[cfg(windows)]
        {
            let pointer = self.windows_current_pointer(service);
            match fs::read_to_string(&pointer) {
                Ok(target) => Ok(Some(PathBuf::from(target.trim()))),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(AppError::State(format!("无法读取当前版本：{error}"))),
            }
        }
        #[cfg(not(windows))]
        {
            let link = self.paths.current.join(service.key());
            match fs::read_link(&link) {
                Ok(target) => Ok(Some(if target.is_absolute() {
                    target
                } else {
                    self.paths.current.join(target)
                })),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
                Err(error) => Err(AppError::State(format!("无法读取当前版本：{error}"))),
            }
        }
    }

    #[cfg(windows)]
    fn windows_current_pointer(&self, service: Service) -> PathBuf {
        self.paths.current.join(format!("{}.path", service.key()))
    }

    #[cfg(windows)]
    fn set_windows_current_pointer(&self, service: Service, target: &Path) -> Result<(), AppError> {
        let pointer = self.windows_current_pointer(service);
        let temporary = pointer.with_extension("path.next");
        fs::write(&temporary, format!("{}\n", target.display()))
            .map_err(|error| AppError::Permission(format!("无法写入当前版本指针：{error}")))?;
        match fs::remove_file(&pointer) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(AppError::Permission(format!(
                    "无法替换当前版本指针：{error}"
                )));
            }
        }
        fs::rename(&temporary, &pointer)
            .map_err(|error| AppError::Permission(format!("无法激活当前版本指针：{error}")))
    }

    /// Keeper 升级前保存数据库和伴随 WAL/SHM，避免未合并写入丢失。
    pub fn backup_keeper_database(&self) -> Result<Option<PathBuf>, AppError> {
        let database_files = ["app.db", "app.db-wal", "app.db-shm"];
        if !database_files
            .iter()
            .any(|name| self.paths.keeper.join(name).is_file())
        {
            return Ok(None);
        }

        let backups = self.paths.keeper.join("migration-backups");
        fs::create_dir_all(&backups)
            .map_err(|error| AppError::Permission(format!("无法创建 Keeper 备份目录：{error}")))?;
        set_private_directory(&backups)?;
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|error| AppError::Internal(format!("无法生成备份时间戳：{error}")))?
            .as_nanos();
        let backup = backups.join(timestamp.to_string());
        fs::create_dir(&backup).map_err(|error| {
            AppError::Permission(format!("无法创建 Keeper 数据库备份：{error}"))
        })?;
        set_private_directory(&backup)?;

        for name in database_files {
            let source = self.paths.keeper.join(name);
            if source.is_file() {
                fs::copy(&source, backup.join(name)).map_err(|error| {
                    AppError::Permission(format!("无法备份 Keeper 数据库：{error}"))
                })?;
            }
        }
        Ok(Some(backup))
    }
}

fn inspect_cache_directory(directory: &Path) -> Result<CacheCleanup, AppError> {
    let mut summary = CacheCleanup::default();
    for entry in fs::read_dir(directory)
        .map_err(|error| AppError::Permission(format!("无法读取下载缓存目录：{error}")))?
    {
        let path = entry
            .map_err(|error| AppError::Permission(format!("无法读取下载缓存内容：{error}")))?
            .path();
        inspect_cache_path(&path, &mut summary)?;
    }
    Ok(summary)
}

fn inspect_cache_path(path: &Path, summary: &mut CacheCleanup) -> Result<(), AppError> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| AppError::Permission(format!("无法读取下载缓存内容：{error}")))?;
    if metadata.file_type().is_symlink() {
        summary.removed_entries += 1;
        return Ok(());
    }
    if metadata.is_dir() {
        *summary = CacheCleanup {
            removed_entries: summary.removed_entries + 1,
            ..*summary
        };
        for entry in fs::read_dir(path)
            .map_err(|error| AppError::Permission(format!("无法读取下载缓存目录：{error}")))?
        {
            inspect_cache_path(
                &entry
                    .map_err(|error| {
                        AppError::Permission(format!("无法读取下载缓存内容：{error}"))
                    })?
                    .path(),
                summary,
            )?;
        }
    } else {
        summary.removed_entries += 1;
        summary.freed_bytes += metadata.len();
    }
    Ok(())
}

fn remove_cache_path<F>(
    path: &Path,
    removed: &mut CacheCleanup,
    on_progress: &mut F,
) -> Result<(), AppError>
where
    F: FnMut(CacheCleanup),
{
    let metadata = fs::symlink_metadata(path)
        .map_err(|error| AppError::Permission(format!("无法读取下载缓存内容：{error}")))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        fs::remove_file(path)
            .map_err(|error| AppError::Permission(format!("无法删除下载缓存文件：{error}")))?;
        removed.removed_entries += 1;
        if !metadata.file_type().is_symlink() {
            removed.freed_bytes += metadata.len();
        }
        on_progress(*removed);
        return Ok(());
    }

    for entry in fs::read_dir(path)
        .map_err(|error| AppError::Permission(format!("无法读取下载缓存目录：{error}")))?
    {
        remove_cache_path(
            &entry
                .map_err(|error| AppError::Permission(format!("无法读取下载缓存内容：{error}")))?
                .path(),
            removed,
            on_progress,
        )?;
    }
    fs::remove_dir(path)
        .map_err(|error| AppError::Permission(format!("无法删除下载缓存目录：{error}")))?;
    removed.removed_entries += 1;
    on_progress(*removed);
    Ok(())
}

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        #[cfg(windows)]
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir(path)
            .map_err(|error| AppError::Permission(format!("无法移除旧版本链接：{error}"))),
        #[cfg(not(windows))]
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(path)
            .map_err(|error| AppError::Permission(format!("无法移除旧版本链接：{error}"))),
        Ok(_) => fs::remove_file(path)
            .map_err(|error| AppError::Permission(format!("无法移除旧版本链接：{error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Permission(format!("无法读取版本链接：{error}"))),
    }
}

#[cfg(unix)]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), AppError> {
    std::os::unix::fs::symlink(target, link)
        .map_err(|error| AppError::Permission(format!("无法创建版本链接：{error}")))
}

#[cfg(unix)]
fn replace_current_link(next: &Path, current: &Path) -> Result<(), AppError> {
    fs::rename(next, current)
        .map_err(|error| AppError::Permission(format!("无法切换当前版本：{error}")))
}

#[cfg(unix)]
fn set_private_directory(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
        .map_err(|error| AppError::Permission(format!("无法设置目录权限：{error}")))
}

#[cfg(not(unix))]
fn set_private_directory(_: &Path) -> Result<(), AppError> {
    Ok(())
}
