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
        let current = self.paths.current.join(service.key());
        let next = self.paths.current.join(format!("{}.next", service.key()));
        remove_path_if_exists(&next)?;
        create_directory_link(target, &next)?;
        replace_current_link(&next, &current)
    }

    pub fn clear_current(&self, service: Service) -> Result<(), AppError> {
        remove_path_if_exists(&self.paths.current.join(service.key()))
    }

    pub fn current_target(&self, service: Service) -> Result<Option<PathBuf>, AppError> {
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

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
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

#[cfg(windows)]
fn create_directory_link(target: &Path, link: &Path) -> Result<(), AppError> {
    std::os::windows::fs::symlink_dir(target, link)
        .map_err(|error| AppError::Permission(format!("无法创建版本链接：{error}")))
}

#[cfg(not(any(unix, windows)))]
fn create_directory_link(_: &Path, _: &Path) -> Result<(), AppError> {
    Err(AppError::State("当前平台不支持版本链接".into()))
}

#[cfg(unix)]
fn replace_current_link(next: &Path, current: &Path) -> Result<(), AppError> {
    fs::rename(next, current)
        .map_err(|error| AppError::Permission(format!("无法切换当前版本：{error}")))
}

#[cfg(not(unix))]
fn replace_current_link(next: &Path, current: &Path) -> Result<(), AppError> {
    remove_path_if_exists(current)?;
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
