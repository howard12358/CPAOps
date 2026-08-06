use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::domain::error::AppError;
use crate::domain::service::Service;

pub mod macos;

pub use macos::MacosPlatform;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CommandOutput {
    pub success: bool,
}

impl CommandOutput {
    pub const fn success() -> Self {
        Self { success: true }
    }

    pub const fn failure() -> Self {
        Self { success: false }
    }
}

pub trait CommandRunner {
    fn run(&self, program: &str, args: &[OsString]) -> Result<CommandOutput, AppError>;
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ProcessCommandRunner;

impl CommandRunner for ProcessCommandRunner {
    fn run(&self, program: &str, args: &[OsString]) -> Result<CommandOutput, AppError> {
        let status = Command::new(program)
            .args(args)
            .status()
            .map_err(|_| AppError::Service("无法执行系统服务管理命令".into()))?;
        Ok(CommandOutput {
            success: status.success(),
        })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceStatus {
    pub managed: bool,
    pub disabled: bool,
    pub listening: bool,
}

pub trait Platform {
    fn check_supported(&self) -> Result<(), AppError>;
    fn check_permissions(&self) -> Result<(), AppError>;
    fn install_services(&self) -> Result<(), AppError>;
    fn remove_services(&self) -> Result<(), AppError>;
    fn start(&self, service: Service) -> Result<(), AppError>;
    fn stop(&self, service: Service) -> Result<(), AppError>;
    fn restart(&self, service: Service) -> Result<(), AppError>;
    fn status(&self, service: Service) -> Result<ServiceStatus, AppError>;
    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError>;
    fn is_port_listening(&self, service: Service) -> Result<bool, AppError>;
    fn configure_firewall(&self) -> Result<(), AppError>;
}
