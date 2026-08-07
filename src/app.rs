use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{Value, json};

use crate::cli::{Command, ProxyAction};
use crate::domain::error::AppError;
use crate::domain::release::{
    ReleaseMetadata, ReleasePlan, ReleasePlatform, ReleaseTransaction, ServiceLifecycle,
};
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::github::GithubClient;
use crate::output::Output;
use crate::platform::{Platform, ServiceStatus};
use crate::progress::{NoProgress, ProgressReporter};
use crate::storage::config::{ConfigStore, GithubTokenStore, ProxyConfig};
use crate::storage::filesystem::RuntimeStore;

pub trait ReleaseProvider {
    fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError>;
    fn download(&self, url: &str, destination: &Path) -> Result<PathBuf, AppError>;
    fn download_with_progress(
        &self,
        url: &str,
        destination: &Path,
        _: &dyn ProgressReporter,
    ) -> Result<PathBuf, AppError> {
        self.download(url, destination)
    }
}

impl ReleaseProvider for GithubClient {
    fn latest_release(&self, service: Service) -> Result<ReleaseMetadata, AppError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| AppError::Internal("无法初始化 GitHub 请求运行时".into()))?;
        runtime.block_on(GithubClient::latest_release(self, service))
    }

    fn download(&self, url: &str, destination: &Path) -> Result<PathBuf, AppError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| AppError::Internal("无法初始化 GitHub 请求运行时".into()))?;
        runtime.block_on(GithubClient::download(self, url, destination))
    }

    fn download_with_progress(
        &self,
        url: &str,
        destination: &Path,
        progress: &dyn ProgressReporter,
    ) -> Result<PathBuf, AppError> {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|_| AppError::Internal("无法初始化 GitHub 请求运行时".into()))?;
        runtime.block_on(GithubClient::download_with_progress(
            self,
            url,
            destination,
            progress,
        ))
    }
}

pub struct App<P, R = GithubClient> {
    paths: RuntimePaths,
    platform: P,
    config: ConfigStore,
    release_provider: R,
    release_platform: Option<ReleasePlatform>,
    progress: Arc<dyn ProgressReporter>,
    interactive_proxy_prompt: bool,
}

impl<P: Platform> App<P, GithubClient> {
    pub fn new(paths: RuntimePaths, platform: P) -> Self {
        let config = ConfigStore::new(paths.clone());
        Self {
            release_provider: GithubClient::new(config.clone())
                .expect("固定 GitHub API 地址必须有效"),
            config,
            paths,
            platform,
            release_platform: None,
            progress: Arc::new(NoProgress),
            interactive_proxy_prompt: false,
        }
    }
}

impl<P: Platform, R: ReleaseProvider> App<P, R> {
    pub fn with_release_provider(
        paths: RuntimePaths,
        platform: P,
        release_provider: R,
        release_platform: ReleasePlatform,
    ) -> Self {
        Self {
            config: ConfigStore::new(paths.clone()),
            paths,
            platform,
            release_provider,
            release_platform: Some(release_platform),
            progress: Arc::new(NoProgress),
            interactive_proxy_prompt: false,
        }
    }

    pub fn with_progress(mut self, progress: Arc<dyn ProgressReporter>) -> Self {
        self.progress = progress;
        self
    }

    pub fn with_interactive_proxy_prompt(mut self, enabled: bool) -> Self {
        self.interactive_proxy_prompt = enabled;
        self
    }

    pub fn run(&self, command: &Command) -> Result<Output, AppError> {
        let result = match command {
            Command::Install => self.install(),
            Command::Update { service } => self.update(service.as_deref()),
            Command::Upgrade { .. } => {
                Err(AppError::Internal("升级命令必须由 CLI 入口处理".into()))
            }
            Command::Rollback { service, version } => self.rollback(service, version),
            Command::Path { shell, .. } => Ok(Output::success_with_data(
                if *shell {
                    shell_change_directory(&self.paths.root)
                } else {
                    self.paths.root.display().to_string()
                },
                json!({ "root": self.paths.root.display().to_string() }),
            )),
            Command::Status => self.status(),
            Command::Logs { service, lines, .. } => {
                self.logs(ServiceCatalog::resolve(service)?, *lines)
            }
            Command::Start { service } => self.start(service.as_deref()),
            Command::Stop { service } => self.stop(service.as_deref()),
            Command::Restart { service } => self.restart(service.as_deref()),
            Command::Proxy { action } => self.proxy(action),
            Command::Auth { .. } => Err(AppError::Internal("认证命令必须由 CLI 入口处理".into())),
            Command::Uninstall { purge } => self.uninstall(*purge),
        };
        self.progress.clear();
        result
    }

    pub fn log_follower(&self, service_name: &str) -> Result<LogFollower, AppError> {
        let service = ServiceCatalog::resolve(service_name)?;
        Ok(LogFollower::new(log_paths(&self.paths, service)))
    }

    fn status(&self) -> Result<Output, AppError> {
        self.require_installed()?;
        self.platform.check_supported()?;
        self.platform.check_permissions()?;
        let services = [Service::Cli, Service::Keeper]
            .into_iter()
            .map(|service| self.status_entry(service))
            .collect::<Result<Vec<_>, _>>()?;
        Ok(Output::success_with_data(
            "服务状态",
            json!({
                "root": self.paths.root.display().to_string(),
                "services": services,
            }),
        ))
    }

    fn install(&self) -> Result<Output, AppError> {
        self.platform.check_supported()?;
        self.platform.check_permissions()?;
        RuntimeStore::new(self.paths.clone()).ensure_layout()?;
        self.initialize_config_for_install()?;
        self.config.validate()?;
        self.configure_proxy_before_release()?;

        let releases = [Service::Cli, Service::Keeper]
            .into_iter()
            .map(|service| {
                self.prepare_release(service)
                    .map(|version| (service, version))
            })
            .collect::<Result<Vec<_>, _>>()?;
        self.platform.install_services()?;
        let results = releases
            .into_iter()
            .map(|(service, version)| {
                workflow_result(service, self.activate_release(service, &version))
            })
            .collect();
        workflow_output("安装", results)
    }

    fn update(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        self.config.validate()?;
        let services = resolve_services(service_name)?;
        let mut results = Vec::with_capacity(services.len());
        for service in services {
            let result = self.prepare_release(service).and_then(|version| {
                if self.current_version(service)?.as_deref() == Some(version.as_str()) {
                    Ok((version, "up_to_date"))
                } else {
                    self.activate_release(service, &version)?;
                    Ok((version, "updated"))
                }
            });
            results.push(update_workflow_result(service, result));
        }
        workflow_output("更新", results)
    }

    fn rollback(&self, service_name: &str, version: &str) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        let service = ServiceCatalog::resolve(service_name)?;
        self.activate_release(service, version)?;
        Ok(Output::success_with_data(
            "版本已回滚",
            json!({ "service": service.key(), "version": version }),
        ))
    }

    fn prepare_release(&self, service: Service) -> Result<String, AppError> {
        self.progress
            .stage(&format!("查询 {} Release", service.key()));
        let metadata = self.release_provider.latest_release(service)?;
        let release_platform = self
            .release_platform
            .map(Ok)
            .unwrap_or_else(ReleasePlatform::current)?;
        let plan = ReleasePlan::from_metadata(service, &metadata, release_platform)?;
        let transaction = ReleaseTransaction::new(self.paths.clone());
        if transaction.verified_release(service, &plan.tag)?.is_some() {
            return Ok(plan.tag);
        }

        let asset_name = release_asset_name(&plan.asset.name)?;
        let checksum_name = release_asset_name(&plan.checksums.name)?;
        let download_directory = self.paths.downloads.join(service.key()).join(&plan.tag);
        let archive = download_directory.join(asset_name);
        let checksums = download_directory.join(checksum_name);
        self.progress.stage(&format!("下载 {}", service.key()));
        self.release_provider.download_with_progress(
            &plan.asset.url,
            &archive,
            self.progress.as_ref(),
        )?;
        self.release_provider.download_with_progress(
            &plan.checksums.url,
            &checksums,
            self.progress.as_ref(),
        )?;
        self.progress
            .stage(&format!("校验并解压 {}", service.key()));
        transaction.stage_verified_archive(
            service,
            &plan.tag,
            &archive,
            &checksums,
            &plan.asset.name,
        )?;
        Ok(plan.tag)
    }

    fn activate_release(&self, service: Service, version: &str) -> Result<(), AppError> {
        let transaction = ReleaseTransaction::new(self.paths.clone());
        let mut lifecycle = PlatformLifecycle {
            platform: &self.platform,
            paths: &self.paths,
        };
        transaction.activate(service, version, &mut lifecycle)
    }

    fn initialize_config_for_install(&self) -> Result<(), AppError> {
        if self.config.is_initialized() {
            return Ok(());
        }
        let management_key = install_secret("CPA_MANAGEMENT_KEY", "请输入 CPA 管理密钥：")?;
        let keeper_login_password =
            install_secret("KEEPER_LOGIN_PASSWORD", "请输入 Keeper 登录密码：")?;
        self.config
            .initialize(&management_key, &keeper_login_password)
    }

    fn configure_proxy_before_release(&self) -> Result<(), AppError> {
        if !self.interactive_proxy_prompt || self.config.load_proxy()?.is_some() {
            return Ok(());
        }

        eprint!("未配置下载代理，是否现在配置？[y/N] ");
        io::stderr()
            .flush()
            .map_err(|_| AppError::Internal("无法写入代理确认提示".into()))?;
        let mut confirmation = String::new();
        io::stdin()
            .read_line(&mut confirmation)
            .map_err(|_| AppError::Internal("无法读取代理确认输入".into()))?;
        if !matches!(confirmation.trim(), "y" | "Y" | "yes" | "YES") {
            return Ok(());
        }

        eprint!("请输入代理地址或 export 代理环境变量：");
        io::stderr()
            .flush()
            .map_err(|_| AppError::Internal("无法写入代理输入提示".into()))?;
        let mut url = String::new();
        io::stdin()
            .read_line(&mut url)
            .map_err(|_| AppError::Internal("无法读取代理地址".into()))?;
        let proxy = ProxyConfig::from_url(&url)?;
        self.config.save_proxy(&proxy)
    }

    fn status_entry(&self, service: Service) -> Result<Value, AppError> {
        let status = self.platform.status(service)?;
        let definition = ServiceCatalog::definition(service);
        Ok(json!({
            "service": service.key(),
            "status": logical_status(status),
            "managed": status.managed,
            "disabled": status.disabled,
            "listening": status.listening,
            "port": definition.port,
            "version": self.current_version(service)?,
        }))
    }

    fn current_version(&self, service: Service) -> Result<Option<String>, AppError> {
        let current = self.paths.current.join(service.key());
        let target = match fs::read_link(&current) {
            Ok(target) => target,
            Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Ok(None),
        };
        Ok(target
            .file_name()
            .and_then(|name| name.to_str())
            .map(ToOwned::to_owned))
    }

    fn logs(&self, service: Service, lines: usize) -> Result<Output, AppError> {
        self.require_installed()?;
        let paths = log_paths(&self.paths, service);
        let stdout = tail_lines(&paths[0], lines)?;
        let stderr = tail_lines(&paths[1], lines)?;
        let message = render_logs(&stdout, &stderr);
        Ok(Output::success_with_data(
            message,
            json!({
                "service": service.key(),
                "logs": [
                    { "stream": "stdout", "lines": stdout },
                    { "stream": "stderr", "lines": stderr },
                ],
            }),
        ))
    }

    fn start(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        for service in resolve_services(service_name)? {
            self.require_registered(service)?;
            clear_disabled(&self.paths, service)?;
            self.platform.start(service)?;
        }
        Ok(Output::success("服务已启动"))
    }

    fn stop(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        for service in resolve_services(service_name)? {
            self.require_registered(service)?;
            mark_disabled(&self.paths, service)?;
            self.platform.stop(service)?;
        }
        Ok(Output::success("服务已停止"))
    }

    fn restart(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        for service in resolve_services(service_name)? {
            self.require_registered(service)?;
            clear_disabled(&self.paths, service)?;
            self.platform.restart(service)?;
        }
        Ok(Output::success("服务已重启"))
    }

    fn proxy(&self, action: &ProxyAction) -> Result<Output, AppError> {
        let settings = GithubTokenStore::default_location();
        match action {
            ProxyAction::Show => Ok(Output::success_with_data(
                if settings.load_proxy()?.is_some() {
                    "已配置代理"
                } else {
                    "未配置代理"
                },
                json!({ "configured": settings.load_proxy()?.is_some() }),
            )),
            ProxyAction::Clear => {
                settings.clear_proxy()?;
                Ok(Output::success("已清除代理"))
            }
            ProxyAction::Set => {
                let proxy = proxy_from_environment()?;
                settings.save_proxy(&proxy)?;
                Ok(Output::success("已保存代理"))
            }
        }
    }

    fn uninstall(&self, purge: bool) -> Result<Output, AppError> {
        self.platform.check_supported()?;
        self.platform.check_permissions()?;
        self.platform.remove_services()?;
        if purge {
            confirm_purge()?;
            purge_runtime_root(&self.paths.root)?;
        }
        Ok(Output::success(if purge {
            "已卸载服务并清除运行目录"
        } else {
            "已卸载服务，运行数据已保留"
        }))
    }

    fn require_installed(&self) -> Result<(), AppError> {
        if self.paths.root.is_dir() {
            Ok(())
        } else {
            Err(AppError::State(
                "尚未安装 CPA Stack，请先运行 cpactl install".into(),
            ))
        }
    }

    fn prepare_lifecycle(&self) -> Result<(), AppError> {
        self.require_installed()?;
        self.platform.check_supported()?;
        self.platform.check_permissions()
    }

    fn require_registered(&self, service: Service) -> Result<(), AppError> {
        if self.platform.status(service)?.managed {
            Ok(())
        } else {
            Err(AppError::State(
                "服务未安装，请先运行 cpactl install".into(),
            ))
        }
    }
}

struct PlatformLifecycle<'a, P> {
    platform: &'a P,
    paths: &'a RuntimePaths,
}

impl<P: Platform> ServiceLifecycle for PlatformLifecycle<'_, P> {
    fn replace_current(&mut self, service: Service, release: &Path) -> Result<(), AppError> {
        self.platform.replace_current_link(service, release)
    }

    fn clear_current(&mut self, service: Service) -> Result<(), AppError> {
        RuntimeStore::new(self.paths.clone()).clear_current(service)
    }

    fn is_running(&mut self, service: Service) -> Result<bool, AppError> {
        Ok(self.platform.status(service)?.listening)
    }

    fn start(&mut self, service: Service) -> Result<(), AppError> {
        self.platform.start(service)
    }

    fn stop(&mut self, service: Service) -> Result<(), AppError> {
        self.platform.stop(service)
    }

    fn restart(&mut self, service: Service) -> Result<(), AppError> {
        self.platform.restart(service)
    }

    fn is_healthy(&mut self, service: Service) -> Result<bool, AppError> {
        self.platform.is_port_listening(service)
    }

    fn wait_for_healthy(&mut self, service: Service) -> Result<bool, AppError> {
        self.platform.wait_for_port(service)
    }
}

fn workflow_result(service: Service, result: Result<(), AppError>) -> Value {
    match result {
        Ok(()) => json!({ "service": service.key(), "ok": true }),
        Err(error) => json!({
            "service": service.key(),
            "ok": false,
            "code": error.exit_code(),
            "message": error.to_string(),
        }),
    }
}

fn update_workflow_result(
    service: Service,
    result: Result<(String, &'static str), AppError>,
) -> Value {
    match result {
        Ok((version, state)) => json!({
            "service": service.key(),
            "ok": true,
            "version": version,
            "state": state,
        }),
        Err(error) => json!({
            "service": service.key(),
            "ok": false,
            "code": error.exit_code(),
            "message": error.to_string(),
        }),
    }
}

fn workflow_output(action: &str, results: Vec<Value>) -> Result<Output, AppError> {
    let failures = results
        .iter()
        .filter(|result| result["ok"] == Value::Bool(false))
        .collect::<Vec<_>>();
    if failures.is_empty() {
        return Ok(Output::success_with_data(
            format!("{action}完成"),
            json!({ "services": results }),
        ));
    }
    let code = failures
        .iter()
        .filter_map(|result| result["code"].as_u64())
        .max()
        .and_then(|code| u8::try_from(code).ok())
        .unwrap_or(7);
    Ok(Output::failure_with_data(
        code,
        format!("{action}部分失败，已分别保留每项结果"),
        json!({ "services": results }),
    ))
}

fn release_asset_name(name: &str) -> Result<&str, AppError> {
    let path = Path::new(name);
    if name.contains(['/', '\\'])
        || path.components().count() != 1
        || path.file_name().and_then(|part| part.to_str()) != Some(name)
    {
        return Err(AppError::Verification("Release 资产文件名无效".into()));
    }
    Ok(name)
}

#[cfg(unix)]
fn shell_change_directory(path: &Path) -> String {
    let path = path.display().to_string().replace('\'', "'\"'\"'");
    format!("cd -- '{path}'")
}

#[cfg(windows)]
fn shell_change_directory(path: &Path) -> String {
    let path = path.display().to_string().replace('\'', "''");
    format!("Set-Location -LiteralPath '{path}'")
}

#[cfg(not(any(unix, windows)))]
fn shell_change_directory(path: &Path) -> String {
    path.display().to_string()
}

fn install_secret(environment: &str, prompt: &str) -> Result<String, AppError> {
    if let Some(value) = std::env::var_os(environment) {
        return Ok(value.to_string_lossy().into_owned());
    }
    if !io::stdin().is_terminal() {
        return Err(AppError::Usage(format!(
            "首次安装请设置 {environment} 环境变量，或在交互式终端中运行"
        )));
    }
    eprint!("{prompt}");
    io::stderr()
        .flush()
        .map_err(|_| AppError::Internal("无法写入私密配置提示".into()))?;
    rpassword::read_password().map_err(|_| AppError::Internal("无法读取私密配置值".into()))
}

fn resolve_services(name: Option<&str>) -> Result<Vec<Service>, AppError> {
    match name {
        Some(name) => Ok(vec![ServiceCatalog::resolve(name)?]),
        None => Ok(vec![Service::Cli, Service::Keeper]),
    }
}

fn logical_status(status: ServiceStatus) -> &'static str {
    if !status.managed {
        "未安装"
    } else if status.disabled {
        "已禁用"
    } else if status.listening {
        "运行中"
    } else {
        "已停止"
    }
}

fn log_paths(paths: &RuntimePaths, service: Service) -> Vec<PathBuf> {
    let prefix = ServiceCatalog::definition(service).log_prefix;
    vec![
        paths.logs.join(format!("{prefix}.out.log")),
        paths.logs.join(format!("{prefix}.err.log")),
    ]
}

fn tail_lines(path: &Path, line_limit: usize) -> Result<Vec<String>, AppError> {
    let contents = match fs::read_to_string(path) {
        Ok(contents) => contents,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(_) => return Err(AppError::State("无法读取服务日志".into())),
    };
    let lines = contents.lines().collect::<Vec<_>>();
    let start = lines.len().saturating_sub(line_limit);
    Ok(lines
        .into_iter()
        .skip(start)
        .map(ToOwned::to_owned)
        .collect())
}

fn render_logs(stdout: &[String], stderr: &[String]) -> String {
    let mut output = String::new();
    for (label, lines) in [("stdout", stdout), ("stderr", stderr)] {
        for line in lines {
            output.push_str(label);
            output.push_str(": ");
            output.push_str(line);
            output.push('\n');
        }
    }
    if output.is_empty() {
        "暂无日志".into()
    } else {
        output.trim_end().into()
    }
}

fn mark_disabled(paths: &RuntimePaths, service: Service) -> Result<(), AppError> {
    fs::create_dir_all(&paths.state).map_err(|_| AppError::State("无法创建服务状态目录".into()))?;
    fs::write(paths.disabled_file(service), b"disabled\n")
        .map_err(|_| AppError::State("无法写入服务停用标记".into()))
}

fn clear_disabled(paths: &RuntimePaths, service: Service) -> Result<(), AppError> {
    match fs::remove_file(paths.disabled_file(service)) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::State("无法清除服务停用标记".into())),
    }
}

fn proxy_from_environment() -> Result<ProxyConfig, AppError> {
    let mut values = BTreeMap::new();
    for (key, env_key) in [
        ("http_proxy", "HTTP_PROXY"),
        ("https_proxy", "HTTPS_PROXY"),
        ("all_proxy", "ALL_PROXY"),
    ] {
        if let Some(value) = std::env::var_os(key).or_else(|| std::env::var_os(env_key)) {
            values.insert(key, value.to_string_lossy().into_owned());
        }
    }
    if values.is_empty() {
        return Err(AppError::Usage(
            "请先设置 http_proxy、https_proxy 或 all_proxy 环境变量".into(),
        ));
    }
    let assignments = values
        .into_iter()
        .map(|(key, value)| format!("{key}={value}"))
        .collect::<Vec<_>>()
        .join(" ");
    ProxyConfig::parse(&assignments)
}

fn confirm_purge() -> Result<(), AppError> {
    if !io::stdin().is_terminal() {
        return Err(AppError::Usage("--purge 仅允许在交互式终端中执行".into()));
    }
    eprint!("此操作将删除 CPA Stack 运行目录。输入 DELETE 确认：");
    io::stderr()
        .flush()
        .map_err(|_| AppError::Internal("无法写入确认提示".into()))?;
    let mut confirmation = String::new();
    io::stdin()
        .read_line(&mut confirmation)
        .map_err(|_| AppError::Internal("无法读取确认输入".into()))?;
    if confirmation.trim() != "DELETE" {
        return Err(AppError::Usage("未输入 DELETE，已取消清除运行目录".into()));
    }
    Ok(())
}

fn purge_runtime_root(root: &Path) -> Result<(), AppError> {
    let root = root
        .canonicalize()
        .map_err(|_| AppError::State("运行目录不存在，无法清除".into()))?;
    if root.parent().is_none() || !root.join("config").is_dir() || !root.join("releases").is_dir() {
        return Err(AppError::Usage("拒绝清除未经验证的运行目录".into()));
    }
    fs::remove_dir_all(root).map_err(|_| AppError::Permission("无法清除运行目录".into()))
}

pub struct LogFollower {
    offsets: BTreeMap<PathBuf, u64>,
}

impl LogFollower {
    pub fn new(paths: Vec<PathBuf>) -> Self {
        let offsets = paths
            .into_iter()
            .map(|path| {
                let offset = fs::metadata(&path)
                    .map(|metadata| metadata.len())
                    .unwrap_or(0);
                (path, offset)
            })
            .collect();
        Self { offsets }
    }

    pub fn poll(&mut self) -> Result<Vec<String>, AppError> {
        let mut appended = Vec::new();
        for (path, offset) in &mut self.offsets {
            let contents = match fs::read(path) {
                Ok(contents) => contents,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(_) => return Err(AppError::State("无法读取服务日志".into())),
            };
            let start = usize::try_from(*offset)
                .ok()
                .filter(|offset| *offset <= contents.len())
                .unwrap_or(0);
            *offset = u64::try_from(contents.len()).unwrap_or(u64::MAX);
            let added = std::str::from_utf8(&contents[start..])
                .map_err(|_| AppError::State("服务日志不是有效 UTF-8".into()))?;
            appended.extend(added.lines().map(ToOwned::to_owned));
        }
        Ok(appended)
    }
}
