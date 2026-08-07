use std::env;
use std::fmt;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
#[cfg(unix)]
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use url::Url;

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;

const REQUIRED_PLACEHOLDER: &str = "__REQUIRED__";
const CPA_CONFIG_TEMPLATE: &str = include_str!("../../config/cpa.config.yaml.example");
const KEEPER_ENV_TEMPLATE: &str = include_str!("../../config/keeper.env.example");
#[cfg(unix)]
static TEMPORARY_FILE_COUNTER: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug)]
pub struct ConfigStore {
    paths: RuntimePaths,
}

#[derive(Clone, Debug)]
pub struct GithubTokenStore {
    path: PathBuf,
}

impl GithubTokenStore {
    pub fn default_location() -> Self {
        let config = if cfg!(target_os = "windows") {
            env::var_os("LOCALAPPDATA")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from(r"C:\\Users\\Default\\AppData\\Local"))
                .join("CPAStack/config")
        } else {
            env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("."))
                .join("Library/Application Support/cpa-stack/config")
        };
        Self::at(config.join("github-token"))
    }

    pub const fn at(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn save(&self, token: &str) -> Result<(), AppError> {
        validate_secret_input(token)?;
        let directory = self
            .path
            .parent()
            .ok_or_else(|| AppError::Internal("GitHub Token 路径无效".into()))?;
        fs::create_dir_all(directory).map_err(|error| {
            AppError::Permission(format!("无法创建 GitHub Token 目录：{error}"))
        })?;
        set_private_directory(directory)?;
        write_private_file(&self.path, token)
    }

    pub fn load(&self) -> Result<Option<String>, AppError> {
        if !self.path.exists() {
            return Ok(None);
        }
        ensure_private_file(&self.path)?;
        let token = fs::read_to_string(&self.path)
            .map_err(|_| AppError::State("GitHub Token 无法读取".into()))?;
        let token = token.trim();
        Ok((!token.is_empty()).then(|| token.into()))
    }

    pub fn clear(&self) -> Result<(), AppError> {
        match fs::remove_file(&self.path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Permission(format!(
                "无法清除 GitHub Token：{error}"
            ))),
        }
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
        let directory = self
            .path
            .parent()
            .ok_or_else(|| AppError::Internal("GitHub 配置路径无效".into()))?;
        fs::create_dir_all(directory)
            .map_err(|error| AppError::Permission(format!("无法创建 GitHub 配置目录：{error}")))?;
        set_private_directory(directory)?;
        let contents = toml::to_string(&StoredProxyConfig::from(proxy))
            .map_err(|_| AppError::Internal("代理配置无法序列化".into()))?;
        write_private_file(&self.proxy_path(), &contents)
    }

    pub fn clear_proxy(&self) -> Result<(), AppError> {
        match fs::remove_file(self.proxy_path()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(AppError::Permission(format!("无法清除代理配置：{error}"))),
        }
    }

    fn proxy_path(&self) -> PathBuf {
        self.path.with_file_name("proxy.toml")
    }
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
        if !write_private_file_if_absent(
            &cpa_config,
            &CPA_CONFIG_TEMPLATE.replace(REQUIRED_PLACEHOLDER, management_key),
        )? {
            set_private_file(&cpa_config)?;
        }

        let keeper_env = self.keeper_env_path();
        let keeper_contents = KEEPER_ENV_TEMPLATE
            .replace(
                "CPA_MANAGEMENT_KEY=__REQUIRED__",
                &format!("CPA_MANAGEMENT_KEY={management_key}"),
            )
            .replace(
                "LOGIN_PASSWORD=__REQUIRED__",
                &format!("LOGIN_PASSWORD={keeper_login_password}"),
            );
        if !write_private_file_if_absent(&keeper_env, &keeper_contents)? {
            set_private_file(&keeper_env)?;
        }

        Ok(())
    }

    pub fn is_initialized(&self) -> bool {
        self.cpa_config_path().is_file() && self.keeper_env_path().is_file()
    }

    pub fn validate(&self) -> Result<(), AppError> {
        let cpa_config = self.cpa_config_path();
        let cpa_contents = read_private_config(&cpa_config)?;
        if cpa_contents.contains(REQUIRED_PLACEHOLDER) {
            return Err(AppError::State("私密配置包含必填占位符".into()));
        }
        validate_cpa_port(&cpa_contents)?;

        let keeper_env = self.keeper_env_path();
        let keeper_contents = read_private_config(&keeper_env)?;
        if keeper_contents.contains(REQUIRED_PLACEHOLDER) {
            return Err(AppError::State("私密配置包含必填占位符".into()));
        }
        validate_keeper_port(&keeper_contents)?;

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

    fn cpa_config_path(&self) -> PathBuf {
        self.paths.config.join("config.yaml")
    }

    fn keeper_env_path(&self) -> PathBuf {
        self.paths.config.join("keeper.env")
    }

    fn proxy_path(&self) -> PathBuf {
        self.paths.config.join("proxy.toml")
    }
}

#[derive(Deserialize)]
struct CpaConfig {
    port: u16,
}

fn read_private_config(path: &Path) -> Result<String, AppError> {
    let contents = fs::read_to_string(path)
        .map_err(|_| AppError::State("私密配置文件不存在或无法读取".into()))?;
    ensure_private_file(path)?;
    Ok(contents)
}

fn validate_cpa_port(contents: &str) -> Result<(), AppError> {
    let config: CpaConfig =
        serde_yaml::from_str(contents).map_err(|_| AppError::State("CPA 配置 port 无效".into()))?;
    if config.port == 0 {
        return Err(AppError::State("CPA 配置 port 必须是 1 到 65535".into()));
    }
    Ok(())
}

fn validate_keeper_port(contents: &str) -> Result<(), AppError> {
    let mut app_port = None;
    for (key, value) in contents.lines().filter_map(|line| line.split_once('=')) {
        if key == "APP_PORT" && app_port.replace(value).is_some() {
            return Err(AppError::State("Keeper 配置包含重复 APP_PORT".into()));
        }
    }
    let app_port = app_port
        .ok_or_else(|| AppError::State("Keeper 配置缺少 APP_PORT".into()))?
        .parse::<u16>()
        .map_err(|_| AppError::State("Keeper 配置 APP_PORT 无效".into()))?;
    if app_port == 0 {
        return Err(AppError::State(
            "Keeper 配置 APP_PORT 必须是 1 到 65535".into(),
        ));
    }
    Ok(())
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
    pub fn from_url(url: &str) -> Result<Self, AppError> {
        let url = url.trim();
        if url.is_empty() {
            return Err(AppError::Usage("代理地址不能为空".into()));
        }
        if url.starts_with("export ")
            || url.starts_with("http_proxy=")
            || url.starts_with("https_proxy=")
            || url.starts_with("all_proxy=")
        {
            return Self::parse(url);
        }
        if url.starts_with("$env:") {
            return Self::parse(&powershell_proxy_assignments(url)?);
        }
        Self::parse(&format!("all_proxy={url}"))
    }

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

    pub(crate) fn urls(&self) -> impl Iterator<Item = (&str, &str)> {
        [
            ("https", self.https_proxy.as_deref()),
            ("http", self.http_proxy.as_deref()),
            ("all", self.all_proxy.as_deref()),
        ]
        .into_iter()
        .filter_map(|(scheme, url)| url.map(|url| (scheme, url)))
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

fn powershell_proxy_assignments(input: &str) -> Result<String, AppError> {
    input
        .split(';')
        .filter(|assignment| !assignment.trim().is_empty())
        .map(|assignment| {
            let assignment = assignment.trim();
            let assignment = assignment
                .strip_prefix("$env:")
                .or_else(|| assignment.strip_prefix("$Env:"))
                .ok_or_else(|| AppError::Usage("PowerShell 代理配置格式无效".into()))?;
            let (key, value) = assignment
                .split_once('=')
                .ok_or_else(|| AppError::Usage("PowerShell 代理配置格式无效".into()))?;
            let key = key.to_ascii_lowercase();
            let value = value.trim().trim_matches(['\'', '"']);
            Ok(format!("{key}={value}"))
        })
        .collect::<Result<Vec<_>, AppError>>()
        .map(|assignments| assignments.join(" "))
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

fn write_private_file_if_absent(path: &Path, contents: &str) -> Result<bool, AppError> {
    match open_private_file(path, true) {
        Ok(file) => {
            write_contents(file, path, contents)?;
            Ok(true)
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => Ok(false),
        Err(error) => Err(AppError::Permission(format!("无法写入私密配置：{error}"))),
    }
}

#[cfg(not(unix))]
fn write_private_file(path: &Path, contents: &str) -> Result<(), AppError> {
    let file = open_private_file(path, false)
        .map_err(|error| AppError::Permission(format!("无法写入私密配置：{error}")))?;
    write_contents(file, path, contents)
}

#[cfg(unix)]
fn write_private_file(path: &Path, contents: &str) -> Result<(), AppError> {
    let directory = path
        .parent()
        .ok_or_else(|| AppError::Internal("私密配置路径无效".into()))?;
    let file_name = path
        .file_name()
        .ok_or_else(|| AppError::Internal("私密配置路径无效".into()))?
        .to_string_lossy();

    for _ in 0..16 {
        let counter = TEMPORARY_FILE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let temporary_path = directory.join(format!(
            ".{file_name}.{}.{}.tmp",
            std::process::id(),
            counter
        ));
        match open_private_file(&temporary_path, true) {
            Ok(file) => {
                if let Err(error) = write_contents(file, &temporary_path, contents) {
                    let _ = fs::remove_file(&temporary_path);
                    return Err(error);
                }
                if let Err(error) = fs::rename(&temporary_path, path) {
                    let _ = fs::remove_file(&temporary_path);
                    return Err(AppError::Permission(format!("无法替换私密配置：{error}")));
                }
                return Ok(());
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(AppError::Permission(format!("无法写入私密配置：{error}"))),
        }
    }

    Err(AppError::Internal("无法创建私密配置临时文件".into()))
}

fn write_contents(mut file: fs::File, path: &Path, contents: &str) -> Result<(), AppError> {
    file.write_all(contents.as_bytes())
        .map_err(|error| AppError::Permission(format!("无法写入私密配置：{error}")))?;
    set_private_file(path)
}

fn open_private_file(path: &Path, create_new: bool) -> std::io::Result<fs::File> {
    let mut options = fs::OpenOptions::new();
    options.write(true);
    if create_new {
        options.create_new(true);
    } else {
        options.create(true).truncate(true);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;

        options.mode(0o600);
    }
    options.open(path)
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
