use std::fmt;
use std::fs;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;

const REQUIRED_PLACEHOLDER: &str = "__REQUIRED__";
const CPA_CONFIG_TEMPLATE: &str = include_str!("../../config/cpa.config.yaml.example");
const KEEPER_ENV_TEMPLATE: &str = include_str!("../../config/keeper.env.example");

#[derive(Clone, Debug)]
pub struct ConfigStore {
    paths: RuntimePaths,
}

impl ConfigStore {
    pub const fn new(paths: RuntimePaths) -> Self {
        Self { paths }
    }

    pub fn initialize(
        &self,
        management_key: &str,
        keeper_login_password: &str,
    ) -> Result<(), AppError> {
        validate_secret_input(management_key)?;
        validate_secret_input(keeper_login_password)?;

        fs::create_dir_all(&self.paths.config)
            .map_err(|error| AppError::Permission(format!("无法创建配置目录：{error}")))?;
        set_private_directory(&self.paths.config)?;

        let cpa_config = self.cpa_config_path();
        if cpa_config.exists() {
            set_private_file(&cpa_config)?;
        } else {
            let contents = CPA_CONFIG_TEMPLATE.replace(REQUIRED_PLACEHOLDER, management_key);
            write_private_file(&cpa_config, &contents)?;
        }

        let keeper_env = self.keeper_env_path();
        if keeper_env.exists() {
            set_private_file(&keeper_env)?;
        } else {
            let contents = KEEPER_ENV_TEMPLATE
                .replace(
                    "CPA_MANAGEMENT_KEY=__REQUIRED__",
                    &format!("CPA_MANAGEMENT_KEY={management_key}"),
                )
                .replace(
                    "LOGIN_PASSWORD=__REQUIRED__",
                    &format!("LOGIN_PASSWORD={keeper_login_password}"),
                );
            write_private_file(&keeper_env, &contents)?;
        }

        Ok(())
    }

    pub fn validate(&self) -> Result<(), AppError> {
        for path in [self.cpa_config_path(), self.keeper_env_path()] {
            let contents = fs::read_to_string(&path)
                .map_err(|_| AppError::State("私密配置文件不存在或无法读取".into()))?;
            if contents.contains(REQUIRED_PLACEHOLDER) {
                return Err(AppError::State("私密配置包含必填占位符".into()));
            }
            ensure_private_file(&path)?;
        }
        Ok(())
    }

    pub fn load_proxy(&self) -> Result<Option<ProxyConfig>, AppError> {
        let path = self.proxy_path();
        if !path.exists() {
            return Ok(None);
        }
        let contents =
            fs::read_to_string(&path).map_err(|_| AppError::State("代理配置无法读取".into()))?;
        let stored: StoredProxyConfig =
            toml::from_str(&contents).map_err(|_| AppError::State("代理配置格式无效".into()))?;
        let proxy = ProxyConfig::from(stored);
        proxy.validate()?;
        ensure_private_file(&path)?;
        Ok(Some(proxy))
    }

    pub fn save_proxy(&self, proxy: &ProxyConfig) -> Result<(), AppError> {
        proxy.validate()?;
        fs::create_dir_all(&self.paths.config)
            .map_err(|error| AppError::Permission(format!("无法创建配置目录：{error}")))?;
        set_private_directory(&self.paths.config)?;
        let contents = toml::to_string(&StoredProxyConfig::from(proxy))
            .map_err(|_| AppError::Internal("代理配置无法序列化".into()))?;
        write_private_file(&self.proxy_path(), &contents)
    }

    pub fn clear_proxy(&self) -> Result<(), AppError> {
        let path = self.proxy_path();
        match fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Permission(format!("无法清除代理配置：{error}"))),
        }
    }

    pub fn token_present(&self) -> bool {
        fs::read_to_string(self.token_path())
            .map(|token| !token.trim().is_empty())
            .unwrap_or(false)
    }

    fn cpa_config_path(&self) -> PathBuf {
        self.paths.config.join("config.yaml")
    }

    fn keeper_env_path(&self) -> PathBuf {
        self.paths.config.join("keeper.env")
    }

    fn proxy_path(&self) -> PathBuf {
        self.paths.config.join("proxy.toml")
    }

    fn token_path(&self) -> PathBuf {
        self.paths.config.join("github-token")
    }
}

#[derive(Clone)]
pub struct ProxyConfig {
    https_proxy: Option<String>,
    http_proxy: Option<String>,
    all_proxy: Option<String>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct StoredProxyConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    https_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    http_proxy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    all_proxy: Option<String>,
}

impl From<StoredProxyConfig> for ProxyConfig {
    fn from(stored: StoredProxyConfig) -> Self {
        Self {
            https_proxy: stored.https_proxy,
            http_proxy: stored.http_proxy,
            all_proxy: stored.all_proxy,
        }
    }
}

impl From<&ProxyConfig> for StoredProxyConfig {
    fn from(proxy: &ProxyConfig) -> Self {
        Self {
            https_proxy: proxy.https_proxy.clone(),
            http_proxy: proxy.http_proxy.clone(),
            all_proxy: proxy.all_proxy.clone(),
        }
    }
}

impl ProxyConfig {
    pub fn parse(input: &str) -> Result<Self, AppError> {
        let assignments = input.strip_prefix("export ").unwrap_or(input);
        let mut proxy = Self {
            https_proxy: None,
            http_proxy: None,
            all_proxy: None,
        };

        for assignment in assignments.split_whitespace() {
            let (key, value) = assignment
                .split_once('=')
                .ok_or_else(|| AppError::Usage("代理配置格式无效".into()))?;
            let value = validate_proxy_url(value)?;
            let slot = match key {
                "https_proxy" => &mut proxy.https_proxy,
                "http_proxy" => &mut proxy.http_proxy,
                "all_proxy" => &mut proxy.all_proxy,
                _ => {
                    return Err(AppError::Usage(
                        "代理配置仅支持 http_proxy、https_proxy、all_proxy".into(),
                    ));
                }
            };
            if slot.replace(value).is_some() {
                return Err(AppError::Usage("代理配置不能重复设置同一键".into()));
            }
        }

        proxy.validate()?;
        Ok(proxy)
    }

    pub const fn redacted_summary(&self) -> &'static str {
        "已配置代理"
    }

    fn validate(&self) -> Result<(), AppError> {
        let values = [&self.https_proxy, &self.http_proxy, &self.all_proxy];
        if values.iter().all(|value| value.is_none()) {
            return Err(AppError::Usage("代理配置不能为空".into()));
        }
        for value in values.into_iter().flatten() {
            validate_proxy_url(value)?;
        }
        Ok(())
    }
}

impl fmt::Debug for ProxyConfig {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("ProxyConfig(已配置代理)")
    }
}

pub struct Redacted<T>(T);

impl<T> Redacted<T> {
    pub const fn new(value: T) -> Self {
        Self(value)
    }
}

impl<T> fmt::Display for Redacted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("已配置")
    }
}

impl<T> fmt::Debug for Redacted<T> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Redacted(已配置)")
    }
}

fn validate_secret_input(value: &str) -> Result<(), AppError> {
    if value.trim().is_empty()
        || value.contains(['\n', '\r'])
        || value.contains(REQUIRED_PLACEHOLDER)
    {
        return Err(AppError::Usage("私密配置值无效".into()));
    }
    Ok(())
}

fn validate_proxy_url(value: &str) -> Result<String, AppError> {
    let url = Url::parse(value).map_err(|_| AppError::Usage("代理地址无效".into()))?;
    if !matches!(url.scheme(), "http" | "https" | "socks5") || url.host_str().is_none() {
        return Err(AppError::Usage(
            "代理地址必须使用 http、https 或 socks5 协议".into(),
        ));
    }
    Ok(value.into())
}

fn write_private_file(path: &Path, contents: &str) -> Result<(), AppError> {
    fs::write(path, contents)
        .map_err(|error| AppError::Permission(format!("无法写入私密配置：{error}")))?;
    set_private_file(path)
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

#[cfg(unix)]
fn set_private_file(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    fs::set_permissions(path, fs::Permissions::from_mode(0o600))
        .map_err(|error| AppError::Permission(format!("无法设置私密文件权限：{error}")))
}

#[cfg(not(unix))]
fn set_private_file(_: &Path) -> Result<(), AppError> {
    Ok(())
}

#[cfg(unix)]
fn ensure_private_file(path: &Path) -> Result<(), AppError> {
    use std::os::unix::fs::PermissionsExt;

    let mode = fs::metadata(path)
        .map_err(|_| AppError::State("私密配置文件不存在或无法读取".into()))?
        .permissions()
        .mode()
        & 0o777;
    if mode != 0o600 {
        return Err(AppError::Permission("私密配置文件权限必须为 0600".into()));
    }
    Ok(())
}

#[cfg(not(unix))]
fn ensure_private_file(_: &Path) -> Result<(), AppError> {
    Ok(())
}
