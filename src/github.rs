use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

use futures_util::StreamExt;
use reqwest::{Client, Proxy, Response, StatusCode, Url};
use tempfile::NamedTempFile;

use crate::domain::error::AppError;
use crate::domain::release::ReleaseMetadata;
use crate::domain::service::{Service, ServiceCatalog};
use crate::progress::{NoProgress, ProgressReporter};
use crate::storage::config::{ConfigStore, ProxyConfig};

const GITHUB_API_BASE: &str = "https://api.github.com/";
const USER_AGENT: &str = "cpactl";

#[derive(Clone)]
pub struct GithubClient {
    api_base: Url,
    config: ConfigStore,
}

impl GithubClient {
    pub fn new(config: ConfigStore) -> Result<Self, AppError> {
        Self::with_api_base(config, GITHUB_API_BASE)
    }

    pub fn with_api_base(config: ConfigStore, api_base: impl AsRef<str>) -> Result<Self, AppError> {
        let api_base = Url::parse(api_base.as_ref())
            .map_err(|_| AppError::Usage("GitHub API 地址无效".into()))?;
        Ok(Self { api_base, config })
    }

    pub async fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError> {
        let repository = ServiceCatalog::definition(service).repository;
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

    async fn get(&self, url: Url) -> Result<Response, AppError> {
        let client = build_client(self.config.load_proxy()?)?;
        let response = client
            .get(url.clone())
            .send()
            .await
            .map_err(|_| AppError::Network("无法访问 GitHub，请检查网络或代理配置".into()))?;
        if !is_auth_failure(response.status()) {
            return checked_response(response);
        }

        let Some(token) = self.config.load_token()? else {
            return Err(AppError::Network(
                "GitHub 拒绝访问，请配置 GitHub Token".into(),
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
