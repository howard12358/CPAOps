use std::fs;
use std::path::Path;

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;

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
