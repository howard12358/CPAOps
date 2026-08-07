use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;

use futures_util::StreamExt;
use reqwest::{Client, Proxy, Response, StatusCode, Url};
use tempfile::NamedTempFile;

use crate::domain::error::AppError;
use crate::domain::release::ReleaseMetadata;
use crate::domain::service::{Service, ServiceCatalog};
use crate::progress::{NoProgress, ProgressReporter};
use crate::storage::config::{ConfigStore, GithubTokenStore, ProxyConfig};

const GITHUB_API_BASE: &str = "https://api.github.com/";
const GITHUB_DEVICE_CODE_URL: &str = "https://github.com/login/device/code";
const GITHUB_ACCESS_TOKEN_URL: &str = "https://github.com/login/oauth/access_token";
const GITHUB_OAUTH_CLIENT_ID: &str = "Ov23lirbrxh4abfht24N";
const USER_AGENT: &str = "cpactl";

#[derive(Clone)]
pub struct GithubClient {
    api_base: Url,
    token_store: GithubTokenStore,
}

impl GithubClient {
    pub fn new(config: ConfigStore) -> Result<Self, AppError> {
        Self::with_api_base_and_token_store(
            config,
            GITHUB_API_BASE,
            GithubTokenStore::default_location(),
        )
    }

    pub fn with_api_base(config: ConfigStore, api_base: impl AsRef<str>) -> Result<Self, AppError> {
        Self::with_api_base_and_token_store(config, api_base, GithubTokenStore::default_location())
    }

    pub fn with_api_base_and_token_store(
        _config: ConfigStore,
        api_base: impl AsRef<str>,
        token_store: GithubTokenStore,
    ) -> Result<Self, AppError> {
        let api_base = Url::parse(api_base.as_ref())
            .map_err(|_| AppError::Usage("GitHub API 地址无效".into()))?;
        Ok(Self {
            api_base,
            token_store,
        })
    }

    pub async fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError> {
        let repository = ServiceCatalog::definition(service).repository;
        self.latest_release_for(repository).await
    }

    pub async fn latest_release_for(&self, repository: &str) -> Result<ReleaseMetadata, AppError> {
        let endpoint = self
            .api_base
            .join(&format!("repos/{repository}/releases/latest"))
            .map_err(|_| AppError::Internal("无法构造 GitHub Release 请求".into()))?;
        let response = self.get(endpoint).await?;
        response
            .json::<ReleaseMetadata>()
            .await
            .map_err(|_| AppError::Network("GitHub 返回的 Release 数据无效".into()))
    }

    pub async fn download(&self, url: &str, destination: &Path) -> Result<PathBuf, AppError> {
        self.download_with_progress(url, destination, &NoProgress)
            .await
    }

    pub async fn download_with_progress(
        &self,
        url: &str,
        destination: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, AppError> {
        let url = Url::parse(url).map_err(|_| AppError::Network("下载地址无效".into()))?;
        let response = self.get(url).await?;
        persist_download(response, destination, progress).await
    }

    pub fn device_login(&self) -> Result<String, AppError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| AppError::Internal("无法初始化 GitHub 认证运行时".into()))?;
        runtime.block_on(self.device_login_inner())
    }

    async fn device_login_inner(&self) -> Result<String, AppError> {
        eprintln!("正在请求 GitHub 授权…");
        let client = build_client(self.token_store.load_proxy()?)?;
        let response = client
            .post(GITHUB_DEVICE_CODE_URL)
            .header(reqwest::header::ACCEPT, "application/json")
            .form(&[("client_id", GITHUB_OAUTH_CLIENT_ID)])
            .send()
            .await
            .map_err(|_| {
                AppError::Network("无法连接 GitHub 认证服务，请检查网络或代理配置".into())
            })?;
        let device = oauth_response(response).await?;
        let device_code = device
            .device_code
            .ok_or_else(|| AppError::Network("GitHub 认证响应缺少设备代码".into()))?;
        let user_code = device
            .user_code
            .ok_or_else(|| AppError::Network("GitHub 认证响应缺少用户代码".into()))?;
        let verification_uri = device
            .verification_uri
            .ok_or_else(|| AppError::Network("GitHub 认证响应缺少授权地址".into()))?;
        if open_default_browser(&verification_uri) {
            eprintln!("已打开默认浏览器；如未显示页面，请手动打开：{verification_uri}");
        } else {
            eprintln!("无法打开默认浏览器，请手动打开：{verification_uri}");
        }
        eprintln!("输入一次性验证码：{user_code}");
        eprintln!("等待 GitHub 授权完成…");

        let started = std::time::Instant::now();
        let expires_in = device.expires_in.unwrap_or(900);
        let mut interval = device.interval.unwrap_or(5).max(1);
        loop {
            if started.elapsed().as_secs() >= u64::from(expires_in) {
                return Err(AppError::Network(
                    "GitHub 授权已过期，请重新运行 cpactl auth login".into(),
                ));
            }
            std::thread::sleep(std::time::Duration::from_secs(u64::from(interval)));
            let response = client
                .post(GITHUB_ACCESS_TOKEN_URL)
                .header(reqwest::header::ACCEPT, "application/json")
                .form(&[
                    ("client_id", GITHUB_OAUTH_CLIENT_ID),
                    ("device_code", device_code.as_str()),
                    ("grant_type", "urn:ietf:params:oauth:grant-type:device_code"),
                ])
                .send()
                .await
                .map_err(|_| {
                    AppError::Network("无法轮询 GitHub 授权状态，请检查网络或代理配置".into())
                })?;
            let token = oauth_response(response).await?;
            if let Some(access_token) = token.access_token {
                return Ok(access_token);
            }
            match token.error.as_deref() {
                Some("authorization_pending") => continue,
                Some("slow_down") => interval = interval.saturating_add(5),
                Some("expired_token") => {
                    return Err(AppError::Network(
                        "GitHub 授权已过期，请重新运行 cpactl auth login".into(),
                    ));
                }
                Some("access_denied") => {
                    return Err(AppError::Network("GitHub 授权被拒绝".into()));
                }
                _ => return Err(AppError::Network("GitHub 认证失败".into())),
            }
        }
    }

    async fn get(&self, url: Url) -> Result<Response, AppError> {
        let client = build_client(self.token_store.load_proxy()?)?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| AppError::Network("无法访问 GitHub，请检查网络或代理配置".into()))?;
        if !is_auth_failure(response.status()) {
            return checked_response(response);
        }

        let Some(token) = self.token_store.load()? else {
            return Err(AppError::Network(
                "GitHub 拒绝访问，请运行 cpactl auth login 进行认证".into(),
            ));
        };
        let response = client
            .get(url)
            .bearer_auth(token)
            .send()
            .await
            .map_err(|_| AppError::Network("无法访问 GitHub，请检查网络或代理配置".into()))?;
        checked_response(response)
    }
}

fn open_default_browser(url: &str) -> bool {
    let (program, arguments) = browser_command(url);
    Command::new(program)
        .args(arguments)
        .status()
        .is_ok_and(|status| status.success())
}

fn browser_command(url: &str) -> (&'static str, Vec<&str>) {
    #[cfg(target_os = "macos")]
    {
        ("open", vec![url])
    }
    #[cfg(target_os = "windows")]
    {
        ("cmd", vec!["/C", "start", "", url])
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        ("xdg-open", vec![url])
    }
}

#[derive(serde::Deserialize)]
struct OAuthResponse {
    access_token: Option<String>,
    device_code: Option<String>,
    user_code: Option<String>,
    verification_uri: Option<String>,
    expires_in: Option<u32>,
    interval: Option<u32>,
    error: Option<String>,
}

async fn oauth_response(response: Response) -> Result<OAuthResponse, AppError> {
    if !response.status().is_success() {
        return Err(AppError::Network("GitHub 认证请求失败".into()));
    }
    response
        .json()
        .await
        .map_err(|_| AppError::Network("GitHub 认证响应无效".into()))
}

fn build_client(proxy: Option<ProxyConfig>) -> Result<Client, AppError> {
    let mut builder = Client::builder().user_agent(USER_AGENT).no_proxy();
    if let Some(proxy) = proxy {
        for (scheme, url) in proxy.urls() {
            let proxy = match scheme {
                "http" => Proxy::http(url),
                "https" => Proxy::https(url),
                "all" => Proxy::all(url),
                _ => unreachable!("ProxyConfig only exposes supported schemes"),
            }
            .map_err(|_| AppError::Network("代理配置无法应用".into()))?;
            builder = builder.proxy(proxy);
        }
    }
    builder
        .build()
        .map_err(|_| AppError::Network("无法初始化 GitHub 网络客户端".into()))
}

fn is_auth_failure(status: StatusCode) -> bool {
    matches!(status, StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN)
}

fn checked_response(response: Response) -> Result<Response, AppError> {
    if response.status().is_success() {
        Ok(response)
    } else {
        Err(AppError::Network(format!(
            "GitHub 请求失败（HTTP {}）",
            response.status().as_u16()
        )))
    }
}

async fn persist_download(
    response: Response,
    destination: &Path,
    progress: &dyn ProgressReporter,
) -> Result<PathBuf, AppError> {
    let parent = destination
        .parent()
        .ok_or_else(|| AppError::Internal("下载目标路径无效".into()))?;
    fs::create_dir_all(parent).map_err(|_| AppError::Permission("无法创建下载目录".into()))?;
    let mut temporary = NamedTempFile::new_in(parent)
        .map_err(|_| AppError::Permission("无法创建下载临时文件".into()))?;
    let name = destination
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("下载文件");
    progress.begin_download(name, response.content_length());
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| AppError::Network("下载 GitHub 资产失败".into()))?;
        temporary
            .write_all(&chunk)
            .map_err(|_| AppError::Permission("无法写入下载临时文件".into()))?;
        progress.advance(chunk.len() as u64);
    }
    progress.finish_download();
    temporary
        .persist(destination)
        .map_err(|_| AppError::Permission("无法完成下载文件写入".into()))?;
    Ok(destination.into())
}

#[cfg(test)]
mod tests {
    #[cfg(target_os = "macos")]
    #[test]
    fn browser_command_opens_the_verification_url_on_macos() {
        use super::browser_command;

        let (program, arguments) = browser_command("https://github.com/login/device");

        assert_eq!(program, "open");
        assert_eq!(arguments, vec!["https://github.com/login/device"]);
    }
}
