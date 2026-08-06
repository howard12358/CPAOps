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
    if cfg!(target_os = "windows") {
        env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\\ProgramData"))
            .join("CPAStack")
    } else {
        env::var_os("HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("."))
            .join("Library/Application Support/cpa-stack")
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
