use std::collections::BTreeMap;
use std::fs;
use std::io::{self, IsTerminal, Write};
use std::path::{Path, PathBuf};

use serde_json::{Value, json};

use crate::cli::{Command, ProxyAction};
use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::output::Output;
use crate::platform::{Platform, ServiceStatus};
use crate::storage::config::{ConfigStore, ProxyConfig};

pub struct App<P> {
    paths: RuntimePaths,
    platform: P,
    config: ConfigStore,
}

impl<P: Platform> App<P> {
    pub fn new(paths: RuntimePaths, platform: P) -> Self {
        Self {
            config: ConfigStore::new(paths.clone()),
            paths,
            platform,
        }
    }

    pub fn run(&self, command: &Command) -> Result<Output, AppError> {
        match command {
            Command::Install | Command::Update { .. } | Command::Rollback { .. } => {
                Err(AppError::State("该命令将在后续版本中实现".into()))
            }
            Command::Path => Ok(Output::success_with_data(
                "运行目录",
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
            Command::Uninstall { purge } => self.uninstall(*purge),
        }
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
            clear_disabled(&self.paths, service)?;
            self.platform.start(service)?;
        }
        Ok(Output::success("服务已启动"))
    }

    fn stop(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        for service in resolve_services(service_name)? {
            mark_disabled(&self.paths, service)?;
            self.platform.stop(service)?;
        }
        Ok(Output::success("服务已停止"))
    }

    fn restart(&self, service_name: Option<&str>) -> Result<Output, AppError> {
        self.prepare_lifecycle()?;
        for service in resolve_services(service_name)? {
            clear_disabled(&self.paths, service)?;
            self.platform.restart(service)?;
        }
        Ok(Output::success("服务已重启"))
    }

    fn proxy(&self, action: &ProxyAction) -> Result<Output, AppError> {
        match action {
            ProxyAction::Show => Ok(Output::success_with_data(
                if self.config.load_proxy()?.is_some() {
                    "已配置代理"
                } else {
                    "未配置代理"
                },
                json!({ "configured": self.config.load_proxy()?.is_some() }),
            )),
            ProxyAction::Clear => {
                self.config.clear_proxy()?;
                Ok(Output::success("已清除代理"))
            }
            ProxyAction::Set => {
                let proxy = proxy_from_environment()?;
                self.config.save_proxy(&proxy)?;
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
