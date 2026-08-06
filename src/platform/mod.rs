use std::env;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

use crate::domain::error::AppError;
use crate::domain::service::Service;

pub mod macos;
pub mod windows;

pub use macos::MacosPlatform;
pub use windows::WindowsPlatform;

pub enum SystemPlatform {
    #[cfg(target_os = "macos")]
    Macos(Box<MacosPlatform>),
    #[cfg(target_os = "windows")]
    Windows(Box<WindowsPlatform>),
    Unsupported,
}

pub fn native_platform(
    paths: crate::domain::runtime::RuntimePaths,
) -> Result<SystemPlatform, AppError> {
    #[cfg(target_os = "macos")]
    {
        MacosPlatform::new(paths).map(|platform| SystemPlatform::Macos(Box::new(platform)))
    }
    #[cfg(target_os = "windows")]
    {
        Ok(SystemPlatform::Windows(Box::new(WindowsPlatform::new(
            paths,
        ))))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = paths;
        Ok(SystemPlatform::Unsupported)
    }
}

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
        if cfg!(debug_assertions)
            && env::var_os("CPACTL_SMOKE_NO_PLATFORM_COMMANDS").as_deref()
                == Some(std::ffi::OsStr::new("1"))
        {
            return Ok(CommandOutput::success());
        }
        let output = Command::new(program)
            .args(args)
            .output()
            .map_err(|_| AppError::Service("无法执行系统服务管理命令".into()))?;
        Ok(CommandOutput {
            success: output.status.success(),
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

impl Platform for SystemPlatform {
    fn check_supported(&self) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.check_supported(),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.check_supported(),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn check_permissions(&self) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.check_permissions(),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.check_permissions(),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn install_services(&self) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.install_services(),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.install_services(),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn remove_services(&self) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.remove_services(),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.remove_services(),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn start(&self, service: Service) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.start(service),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.start(service),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.stop(service),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.stop(service),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn restart(&self, service: Service) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.restart(service),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.restart(service),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn status(&self, service: Service) -> Result<ServiceStatus, AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.status(service),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.status(service),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.replace_current_link(service, release),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.replace_current_link(service, release),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn is_port_listening(&self, service: Service) -> Result<bool, AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.is_port_listening(service),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.is_port_listening(service),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }

    fn configure_firewall(&self) -> Result<(), AppError> {
        match self {
            #[cfg(target_os = "macos")]
            Self::Macos(platform) => platform.configure_firewall(),
            #[cfg(target_os = "windows")]
            Self::Windows(platform) => platform.configure_firewall(),
            Self::Unsupported => Err(AppError::Usage("当前平台不受支持".into())),
        }
    }
}
