use crate::domain::error::AppError;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Service {
    Cli,
    Keeper,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceDefinition {
    pub service: Service,
    pub repository: &'static str,
    pub port: u16,
    pub macos_binary_name: &'static str,
    pub windows_binary_name: &'static str,
    pub log_prefix: &'static str,
    pub launchd_label: &'static str,
}

pub struct ServiceCatalog;

impl ServiceCatalog {
    pub fn resolve(name: &str) -> Result<Service, AppError> {
        match name {
            "cli" | "cli-proxy-api" => Ok(Service::Cli),
            "keeper" | "cpa-usage-keeper" => Ok(Service::Keeper),
            _ => Err(AppError::Usage("服务必须是 cli 或 keeper".into())),
        }
    }

    pub const fn definition(service: Service) -> ServiceDefinition {
        match service {
            Service::Cli => ServiceDefinition {
                service,
                repository: "router-for-me/CLIProxyAPI",
                port: 8317,
                macos_binary_name: "cli-proxy-api",
                windows_binary_name: "cli-proxy-api.exe",
                log_prefix: "cli-proxy-api",
                launchd_label: "io.cpa-local.cli-proxy-api",
            },
            Service::Keeper => ServiceDefinition {
                service,
                repository: "Willxup/cpa-usage-keeper",
                port: 18080,
                macos_binary_name: "cpa-usage-keeper",
                windows_binary_name: "cpa-usage-keeper.exe",
                log_prefix: "keeper",
                launchd_label: "io.cpa-local.usage-keeper",
            },
        }
    }
}

impl Service {
    pub const fn key(self) -> &'static str {
        match self {
            Self::Cli => "cli-proxy-api",
            Self::Keeper => "cpa-usage-keeper",
        }
    }
}
