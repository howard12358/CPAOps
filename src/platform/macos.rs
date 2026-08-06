use std::env;
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};

#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::platform::{
    CommandOutput, CommandRunner, Platform, ProcessCommandRunner, ServiceStatus,
};

#[derive(Clone, Debug)]
pub struct MacosPlatform<R = ProcessCommandRunner> {
    runner: R,
    paths: RuntimePaths,
    launch_agents: PathBuf,
    user_id: u32,
}

impl MacosPlatform<ProcessCommandRunner> {
    pub fn new(paths: RuntimePaths) -> Result<Self, AppError> {
        let home = env::var_os("HOME")
            .map(PathBuf::from)
            .ok_or_else(|| AppError::State("无法确定当前用户的主目录".into()))?;
        Ok(Self {
            runner: ProcessCommandRunner,
            paths,
            launch_agents: home.join("Library/LaunchAgents"),
            user_id: current_user_id()?,
        })
    }
}

impl<R: CommandRunner> MacosPlatform<R> {
    pub fn with_runner(runner: R, paths: RuntimePaths) -> Self {
        let user_id = current_user_id().unwrap_or(0);
        Self {
            runner,
            launch_agents: paths.root.join("launch-agents"),
            paths,
            user_id,
        }
    }

    pub fn plist_path(&self, service: Service) -> PathBuf {
        self.launch_agents.join(format!(
            "{}.plist",
            ServiceCatalog::definition(service).launchd_label
        ))
    }

    fn domain(&self) -> String {
        format!("gui/{}", self.user_id)
    }

    fn service_target(&self, service: Service) -> String {
        format!(
            "{}/{}",
            self.domain(),
            ServiceCatalog::definition(service).launchd_label
        )
    }

    fn run_required(&self, program: &str, args: Vec<OsString>) -> Result<(), AppError> {
        let CommandOutput { success } = self.runner.run(program, &args)?;
        if success {
            Ok(())
        } else {
            Err(AppError::Service("系统服务管理命令执行失败".into()))
        }
    }

    fn write_plist(&self, service: Service) -> Result<PathBuf, AppError> {
        fs::create_dir_all(&self.launch_agents)
            .map_err(|_| AppError::State("无法创建 LaunchAgent 目录".into()))?;
        let definition = ServiceCatalog::definition(service);
        let wrapper = self.wrapper_path(service);
        let plist = format!(
            concat!(
                "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
                "<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" ",
                "\"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n",
                "<plist version=\"1.0\"><dict>\n",
                "<key>Label</key><string>{label}</string>\n",
                "<key>ProgramArguments</key><array><string>{wrapper}</string></array>\n",
                "<key>WorkingDirectory</key><string>{root}</string>\n",
                "<key>RunAtLoad</key><true/><key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>\n",
                "<key>ThrottleInterval</key><integer>10</integer>\n",
                "<key>StandardOutPath</key><string>{out_log}</string>\n",
                "<key>StandardErrorPath</key><string>{err_log}</string>\n",
                "</dict></plist>\n"
            ),
            label = xml_escape(definition.launchd_label),
            wrapper = xml_escape_path(&wrapper)?,
            root = xml_escape_path(&self.paths.root)?,
            out_log = xml_escape_path(
                &self
                    .paths
                    .logs
                    .join(format!("{}.out.log", definition.log_prefix))
            )?,
            err_log = xml_escape_path(
                &self
                    .paths
                    .logs
                    .join(format!("{}.err.log", definition.log_prefix))
            )?,
        );
        let path = self.plist_path(service);
        fs::write(&path, plist).map_err(|_| AppError::State("无法写入 LaunchAgent 配置".into()))?;
        Ok(path)
    }

    fn wrapper_path(&self, service: Service) -> PathBuf {
        self.paths.bin.join(match service {
            Service::Cli => "run-cli-proxy-api",
            Service::Keeper => "run-cpa-usage-keeper",
        })
    }

    fn write_wrapper(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.bin)
            .map_err(|_| AppError::State("无法创建服务包装器目录".into()))?;
        let definition = ServiceCatalog::definition(service);
        let binary = self
            .paths
            .current
            .join(service.key())
            .join(definition.macos_binary_name);
        let config = match service {
            Service::Cli => self.paths.config.join("config.yaml"),
            Service::Keeper => self.paths.config.join("keeper.env"),
        };
        let argument = match service {
            Service::Cli => "-config",
            Service::Keeper => "-env",
        };
        let script = format!(
            "#!/bin/sh\n[ -f {disabled} ] && exit 0\nexec {binary} {argument} {config}\n",
            disabled = shell_quote_path(&self.paths.disabled_file(service))?,
            binary = shell_quote_path(&binary)?,
            config = shell_quote_path(&config)?,
        );
        let path = self.wrapper_path(service);
        fs::write(&path, script).map_err(|_| AppError::State("无法写入服务包装器".into()))?;
        set_private_executable(&path)?;
        Ok(())
    }

    fn clear_disabled(&self, service: Service) -> Result<(), AppError> {
        match fs::remove_file(self.paths.disabled_file(service)) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(_) => Err(AppError::State("无法清除服务停用标记".into())),
        }
    }

    fn mark_disabled(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.state)
            .map_err(|_| AppError::State("无法创建服务状态目录".into()))?;
        fs::write(self.paths.disabled_file(service), b"disabled\n")
            .map_err(|_| AppError::State("无法写入服务停用标记".into()))
    }

    fn is_registered(&self, service: Service) -> Result<bool, AppError> {
        Ok(self
            .runner
            .run(
                "launchctl",
                &[
                    OsString::from("print"),
                    OsString::from(self.service_target(service)),
                ],
            )?
            .success)
    }

    fn bootout_attempted_services(&self, services: &[Service]) {
        for service in services.iter().rev() {
            let _ = self.runner.run(
                "launchctl",
                &[
                    OsString::from("bootout"),
                    OsString::from(self.service_target(*service)),
                ],
            );
        }
    }
}

impl<R: CommandRunner> Platform for MacosPlatform<R> {
    fn check_supported(&self) -> Result<(), AppError> {
        if env::consts::OS == "macos" && env::consts::ARCH == "aarch64" {
            Ok(())
        } else {
            Err(AppError::Usage(
                "仅支持 macOS Apple Silicon（Darwin arm64）".into(),
            ))
        }
    }

    fn check_permissions(&self) -> Result<(), AppError> {
        Ok(())
    }

    fn install_services(&self) -> Result<(), AppError> {
        let mut attempted_services = Vec::new();
        let result = (|| {
            fs::create_dir_all(&self.paths.logs)
                .map_err(|_| AppError::State("无法创建服务日志目录".into()))?;
            for service in [Service::Cli, Service::Keeper] {
                self.write_wrapper(service)?;
                let plist = self.write_plist(service)?;
                self.run_required(
                    "plutil",
                    vec![OsString::from("-lint"), plist.clone().into_os_string()],
                )?;
                if self.is_registered(service)? {
                    continue;
                }
                attempted_services.push(service);
                self.run_required(
                    "launchctl",
                    vec![
                        OsString::from("bootstrap"),
                        OsString::from(self.domain()),
                        plist.into_os_string(),
                    ],
                )?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.bootout_attempted_services(&attempted_services);
        }
        result
    }

    fn remove_services(&self) -> Result<(), AppError> {
        for service in [Service::Cli, Service::Keeper] {
            let _ = self.runner.run(
                "launchctl",
                &[
                    OsString::from("bootout"),
                    OsString::from(self.service_target(service)),
                ],
            )?;
            remove_if_exists(&self.plist_path(service))?;
            remove_if_exists(&self.wrapper_path(service))?;
        }
        Ok(())
    }

    fn start(&self, service: Service) -> Result<(), AppError> {
        self.clear_disabled(service)?;
        self.run_required(
            "launchctl",
            vec![
                OsString::from("kickstart"),
                OsString::from("-k"),
                OsString::from(self.service_target(service)),
            ],
        )
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        self.mark_disabled(service)?;
        self.run_required(
            "launchctl",
            vec![
                OsString::from("kill"),
                OsString::from(self.service_target(service)),
                OsString::from("SIGTERM"),
            ],
        )
    }

    fn restart(&self, service: Service) -> Result<(), AppError> {
        self.start(service)
    }

    fn status(&self, service: Service) -> Result<ServiceStatus, AppError> {
        let managed = self.is_registered(service)?;
        Ok(ServiceStatus {
            managed,
            disabled: self.paths.disabled_file(service).exists(),
            listening: self.is_port_listening(service)?,
        })
    }

    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        if !release.is_dir() {
            return Err(AppError::State("待激活版本目录不存在".into()));
        }
        fs::create_dir_all(&self.paths.current)
            .map_err(|_| AppError::State("无法创建当前版本目录".into()))?;
        let current = self.paths.current.join(service.key());
        let temporary = self.paths.current.join(format!(".{}.new", service.key()));
        remove_if_exists(&temporary)?;
        create_symlink(release, &temporary)?;
        fs::rename(&temporary, &current).map_err(|_| AppError::State("无法原子切换当前版本".into()))
    }

    fn is_port_listening(&self, service: Service) -> Result<bool, AppError> {
        let port = ServiceCatalog::definition(service).port;
        Ok(self
            .runner
            .run(
                "lsof",
                &[
                    OsString::from("-nP"),
                    OsString::from(format!("-iTCP:{port}")),
                    OsString::from("-sTCP:LISTEN"),
                ],
            )?
            .success)
    }

    fn configure_firewall(&self) -> Result<(), AppError> {
        Ok(())
    }
}

fn current_user_id() -> Result<u32, AppError> {
    let output = std::process::Command::new("id")
        .arg("-u")
        .output()
        .map_err(|_| AppError::Permission("无法确定当前用户 ID".into()))?;
    if !output.status.success() {
        return Err(AppError::Permission("无法确定当前用户 ID".into()));
    }
    std::str::from_utf8(&output.stdout)
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .ok_or_else(|| AppError::Permission("无法确定当前用户 ID".into()))
}

fn xml_escape_path(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(xml_escape)
        .ok_or_else(|| AppError::Usage("运行目录必须是有效的 Unicode 路径".into()))
}

fn xml_escape(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('\"', "&quot;")
        .replace('\'', "&apos;")
}

fn shell_quote_path(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(shell_quote)
        .ok_or_else(|| AppError::Usage("运行目录必须是有效的 Unicode 路径".into()))
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\\"'\\\"'"))
}

fn set_private_executable(path: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(0o700))
            .map_err(|_| AppError::State("无法设置服务包装器权限".into()))
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        Ok(())
    }
}

fn remove_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => {
            fs::remove_dir_all(path).map_err(|_| AppError::State("无法移除平台服务文件".into()))
        }
        Ok(_) => fs::remove_file(path).map_err(|_| AppError::State("无法移除平台服务文件".into())),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(AppError::State("无法读取平台服务文件".into())),
    }
}

fn create_symlink(source: &Path, destination: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        symlink(source, destination).map_err(|_| AppError::State("无法创建当前版本链接".into()))
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        Err(AppError::Service("当前平台不支持 macOS 版本链接".into()))
    }
}
