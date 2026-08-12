use std::env;
use std::path::{Path, PathBuf};

use crate::domain::error::AppError;
use crate::domain::service::Service;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RuntimePaths {
    pub root: PathBuf,
    pub config: PathBuf,
    pub auths: PathBuf,
    pub keeper: PathBuf,
    pub releases: PathBuf,
    pub current: PathBuf,
    pub downloads: PathBuf,
    pub logs: PathBuf,
    pub state: PathBuf,
    pub bin: PathBuf,
    pub tasks: PathBuf,
}

impl RuntimePaths {
    pub fn resolve(root_override: Option<PathBuf>) -> Result<Self, AppError> {
        let root = root_override
            .or_else(|| env::var_os("CPA_STACK_ROOT").map(PathBuf::from))
            .unwrap_or_else(default_root);
        Self::from_root(root)
    }

    pub fn disabled_file(&self, service: Service) -> PathBuf {
        self.state.join(format!("{}.disabled", service.key()))
    }

    pub fn from_root(root: PathBuf) -> Result<Self, AppError> {
        validate_root(&root)?;
        Ok(Self {
            config: root.join("config"),
            auths: root.join("auths"),
            keeper: root.join("keeper"),
            releases: root.join("releases"),
            current: root.join("current"),
            downloads: root.join("downloads"),
            logs: root.join("logs"),
            state: root.join("state"),
            bin: root.join("bin"),
            tasks: root.join("tasks"),
            root,
        })
    }
}

fn default_root() -> PathBuf {
    default_root_for(
        env::consts::OS,
        env::var_os("HOME"),
        env::var_os("ProgramData"),
    )
}

fn default_root_for(
    os: &str,
    home: Option<std::ffi::OsString>,
    program_data: Option<std::ffi::OsString>,
) -> PathBuf {
    match os {
        "windows" => program_data
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"))
            .join("CPAStack"),
        "macos" => home
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/cpa-stack"),
        "linux" => PathBuf::from("/var/lib/cpa-stack"),
        _ => PathBuf::from(".").join("cpa-stack"),
    }
}

fn validate_root(root: &Path) -> Result<(), AppError> {
    if root.as_os_str().is_empty() {
        return Err(AppError::Usage("运行根目录不能为空".into()));
    }
    if root.exists() && root.is_file() {
        return Err(AppError::Usage("运行根目录不能是文件".into()));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn linux_default_root_is_system_level() {
        assert_eq!(
            default_root_for("linux", None, None),
            PathBuf::from("/var/lib/cpa-stack")
        );
    }
}
