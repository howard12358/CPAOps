use std::env;
use std::ffi::OsString;
use std::fs;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpStream};
#[cfg(unix)]
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::domain::service::{Service, ServiceCatalog};
use crate::platform::{
    CommandOutput, CommandRunner, Platform, ProcessCommandRunner, ServiceStatus,
};

#[derive(Clone, Debug)]
pub struct LinuxPlatform<R = ProcessCommandRunner> {
    runner: R,
    paths: RuntimePaths,
    unit_directory: PathBuf,
    user_id: u32,
}

impl LinuxPlatform<ProcessCommandRunner> {
    pub fn new(paths: RuntimePaths) -> Result<Self, AppError> {
        Ok(Self {
            runner: ProcessCommandRunner,
            paths,
            unit_directory: PathBuf::from("/etc/systemd/system"),
            user_id: current_user_id()?,
        })
    }
}

impl<R: CommandRunner> LinuxPlatform<R> {
    pub fn with_runner(runner: R, paths: RuntimePaths) -> Self {
        Self::with_runner_and_user_id(runner, paths, 0)
    }

    pub fn with_runner_and_user_id(runner: R, paths: RuntimePaths, user_id: u32) -> Self {
        Self {
            runner,
            unit_directory: paths.root.join("systemd-system"),
            paths,
            user_id,
        }
    }

    pub fn unit_path(&self, service: Service) -> PathBuf {
        self.unit_directory.join(Self::unit_name(service))
    }

    fn unit_name(service: Service) -> &'static str {
        match service {
            Service::Cli => "cpa-stack-cli-proxy-api.service",
            Service::Keeper => "cpa-stack-usage-keeper.service",
        }
    }

    fn wrapper_path(&self, service: Service) -> PathBuf {
        self.paths.bin.join(match service {
            Service::Cli => "run-cli-proxy-api",
            Service::Keeper => "run-cpa-usage-keeper",
        })
    }

    fn run_required(&self, args: &[&str]) -> Result<(), AppError> {
        let args = args.iter().map(OsString::from).collect::<Vec<_>>();
        let output = self.runner.run("systemctl", &args)?;
        if output.success {
            Ok(())
        } else {
            Err(systemctl_failure(output))
        }
    }

    fn write_wrapper(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.bin).map_err(|error| {
            AppError::Permission(format!("无法创建 Linux 服务包装器目录：{error}"))
        })?;
        let definition = ServiceCatalog::definition(service);
        let binary = self
            .paths
            .current
            .join(service.key())
            .join(definition.unix_binary_name);
        let (argument, config) = match service {
            Service::Cli => ("-config", self.paths.config.join("config.yaml")),
            Service::Keeper => ("-env", self.paths.config.join("keeper.env")),
        };
        let contents = format!(
            "#!/bin/sh\n[ -f {disabled} ] && exit 0\nexec {binary} {argument} {config}\n",
            disabled = shell_quote_path(&self.paths.disabled_file(service))?,
            binary = shell_quote_path(&binary)?,
            config = shell_quote_path(&config)?,
        );
        let path = self.wrapper_path(service);
        fs::write(&path, contents)
            .map_err(|error| AppError::Permission(format!("无法写入 Linux 服务包装器：{error}")))?;
        set_mode(&path, 0o700, "无法设置 Linux 服务包装器权限")
    }

    fn write_unit(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.unit_directory).map_err(|error| {
            AppError::Permission(format!("无法创建 systemd unit 目录：{error}"))
        })?;
        let definition = ServiceCatalog::definition(service);
        let contents = format!(
            concat!(
                "[Unit]\n",
                "Description=CPA Stack {service}\n",
                "After=network-online.target\n",
                "Wants=network-online.target\n\n",
                "[Service]\n",
                "Type=simple\n",
                "ExecStart={wrapper}\n",
                "WorkingDirectory={root}\n",
                "Restart=on-failure\n",
                "RestartSec=10s\n",
                "StandardOutput=append:{out_log}\n",
                "StandardError=append:{err_log}\n\n",
                "[Install]\n",
                "WantedBy=multi-user.target\n"
            ),
            service = service.key(),
            wrapper = systemd_escape_path(&self.wrapper_path(service))?,
            root = systemd_escape_path(&self.paths.root)?,
            out_log = systemd_escape_path(
                &self
                    .paths
                    .logs
                    .join(format!("{}.out.log", definition.log_prefix))
            )?,
            err_log = systemd_escape_path(
                &self
                    .paths
                    .logs
                    .join(format!("{}.err.log", definition.log_prefix))
            )?,
        );
        let path = self.unit_path(service);
        fs::write(&path, contents)
            .map_err(|error| AppError::Permission(format!("无法写入 systemd unit：{error}")))?;
        set_mode(&path, 0o644, "无法设置 systemd unit 权限")
    }

    fn clear_disabled(&self, service: Service) -> Result<(), AppError> {
        remove_file_if_exists(&self.paths.disabled_file(service), "无法清除服务停用标记")
    }

    fn mark_disabled(&self, service: Service) -> Result<(), AppError> {
        fs::create_dir_all(&self.paths.state)
            .map_err(|error| AppError::Permission(format!("无法创建服务状态目录：{error}")))?;
        fs::write(self.paths.disabled_file(service), b"disabled\n")
            .map_err(|error| AppError::Permission(format!("无法写入服务停用标记：{error}")))
    }

    fn is_managed(&self, service: Service) -> Result<bool, AppError> {
        let output = self.runner.run(
            "systemctl",
            &[
                OsString::from("show"),
                OsString::from("--property=LoadState"),
                OsString::from("--value"),
                OsString::from(Self::unit_name(service)),
            ],
        )?;
        Ok(output.success && output.stdout.trim() == "loaded")
    }

    fn cleanup_service_files(&self) {
        for service in [Service::Keeper, Service::Cli] {
            let _ = self.run_required(&["disable", "--now", Self::unit_name(service)]);
            let _ = remove_file_if_exists(&self.unit_path(service), "无法删除 systemd unit");
            let _ = remove_file_if_exists(&self.wrapper_path(service), "无法删除 Linux 服务包装器");
        }
        let _ = self.run_required(&["daemon-reload"]);
    }
}

impl<R: CommandRunner> Platform for LinuxPlatform<R> {
    fn check_supported(&self) -> Result<(), AppError> {
        if env::consts::OS != "linux" || env::consts::ARCH != "x86_64" || !cfg!(target_env = "gnu")
        {
            return Err(AppError::Usage(
                "仅支持使用 glibc 与 systemd 的 Linux AMD64".into(),
            ));
        }
        let output = self.runner.run(
            "systemctl",
            &[
                OsString::from("show"),
                OsString::from("--property=Version"),
                OsString::from("--value"),
            ],
        )?;
        output.success.then_some(()).ok_or_else(|| {
            AppError::Usage("当前 Linux 环境未运行 systemd，无法管理系统服务".into())
        })
    }

    fn check_permissions(&self) -> Result<(), AppError> {
        if self.user_id == 0 {
            Ok(())
        } else {
            Err(AppError::Permission(
                "Linux 系统服务管理需要 root 权限，请使用 sudo cpactl 重新运行命令".into(),
            ))
        }
    }

    fn install_services(&self) -> Result<(), AppError> {
        for path in [&self.paths.logs, &self.paths.state, &self.paths.current] {
            fs::create_dir_all(path).map_err(|error| {
                AppError::Permission(format!("无法创建 Linux 运行目录：{error}"))
            })?;
        }
        let result = (|| {
            for service in [Service::Cli, Service::Keeper] {
                self.write_wrapper(service)?;
                self.write_unit(service)?;
            }
            self.run_required(&["daemon-reload"])?;
            for service in [Service::Cli, Service::Keeper] {
                self.run_required(&["enable", "--now", Self::unit_name(service)])?;
            }
            Ok(())
        })();
        if result.is_err() {
            self.cleanup_service_files();
        }
        result
    }

    fn remove_services(&self) -> Result<(), AppError> {
        for service in [Service::Cli, Service::Keeper] {
            let _ = self.run_required(&["disable", "--now", Self::unit_name(service)]);
            remove_file_if_exists(&self.unit_path(service), "无法删除 systemd unit")?;
            remove_file_if_exists(&self.wrapper_path(service), "无法删除 Linux 服务包装器")?;
        }
        self.run_required(&["daemon-reload"])
    }

    fn start(&self, service: Service) -> Result<(), AppError> {
        self.clear_disabled(service)?;
        self.run_required(&["start", Self::unit_name(service)])
    }

    fn stop(&self, service: Service) -> Result<(), AppError> {
        self.mark_disabled(service)?;
        self.run_required(&["stop", Self::unit_name(service)])
    }

    fn restart(&self, service: Service) -> Result<(), AppError> {
        self.clear_disabled(service)?;
        self.run_required(&["restart", Self::unit_name(service)])
    }

    fn status(&self, service: Service) -> Result<ServiceStatus, AppError> {
        Ok(ServiceStatus {
            managed: self.is_managed(service)?,
            disabled: self.paths.disabled_file(service).exists(),
            listening: self.is_port_listening(service)?,
        })
    }

    fn statuses(&self) -> Result<[(Service, ServiceStatus); 2], AppError> {
        let services = [Service::Cli, Service::Keeper];
        let output = self.runner.run(
            "systemctl",
            &[
                OsString::from("show"),
                OsString::from("--property=Id,LoadState"),
                OsString::from(Self::unit_name(Service::Cli)),
                OsString::from(Self::unit_name(Service::Keeper)),
            ],
        )?;
        let managed = services
            .map(|service| systemd_unit_is_loaded(&output.stdout, Self::unit_name(service)));
        Ok([
            (
                Service::Cli,
                ServiceStatus {
                    managed: managed[0],
                    disabled: self.paths.disabled_file(Service::Cli).exists(),
                    listening: self.is_port_listening(Service::Cli)?,
                },
            ),
            (
                Service::Keeper,
                ServiceStatus {
                    managed: managed[1],
                    disabled: self.paths.disabled_file(Service::Keeper).exists(),
                    listening: self.is_port_listening(Service::Keeper)?,
                },
            ),
        ])
    }

    fn replace_current_link(&self, service: Service, release: &Path) -> Result<(), AppError> {
        if !release.is_dir() {
            return Err(AppError::State("待激活版本目录不存在".into()));
        }
        fs::create_dir_all(&self.paths.current)
            .map_err(|error| AppError::Permission(format!("无法创建当前版本目录：{error}")))?;
        let current = self.paths.current.join(service.key());
        let temporary = self.paths.current.join(format!(".{}.new", service.key()));
        remove_path_if_exists(&temporary)?;
        create_directory_symlink(release, &temporary)?;
        fs::rename(&temporary, &current)
            .map_err(|error| AppError::State(format!("无法原子切换当前版本：{error}")))
    }

    fn is_port_listening(&self, service: Service) -> Result<bool, AppError> {
        let address = SocketAddr::new(
            IpAddr::V4(Ipv4Addr::LOCALHOST),
            ServiceCatalog::definition(service).port,
        );
        Ok(TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok())
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

fn systemctl_failure(output: CommandOutput) -> AppError {
    let raw_diagnostic = if output.stderr.is_empty() {
        output.stdout
    } else {
        output.stderr
    };
    if raw_diagnostic.is_empty() {
        AppError::Service("Linux 服务管理失败：systemctl 未返回诊断，请运行 cpactl doctor".into())
    } else {
        AppError::ServiceDiagnostic {
            message: "Linux 服务管理失败，请使用 sudo cpactl 重新运行并执行 cpactl doctor".into(),
            raw_diagnostic,
        }
    }
}

fn shell_quote_path(path: &Path) -> Result<String, AppError> {
    path.to_str()
        .map(|value| format!("'{}'", value.replace('\'', "'\"'\"'")))
        .ok_or_else(|| AppError::Usage("运行目录必须是有效的 Unicode 路径".into()))
}

fn systemd_escape_path(path: &Path) -> Result<String, AppError> {
    let value = path
        .to_str()
        .ok_or_else(|| AppError::Usage("运行目录必须是有效的 Unicode 路径".into()))?;
    if value.contains(['\n', '\r']) {
        return Err(AppError::Usage("运行目录不能包含换行符".into()));
    }
    Ok(value.replace('%', "%%").replace(' ', "\\x20"))
}

fn remove_file_if_exists(path: &Path, message: &str) -> Result<(), AppError> {
    match fs::remove_file(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::Permission(format!("{message}：{error}"))),
    }
}

fn remove_path_if_exists(path: &Path) -> Result<(), AppError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_dir() => fs::remove_dir_all(path)
            .map_err(|error| AppError::State(format!("无法移除旧版本目录：{error}"))),
        Ok(_) => fs::remove_file(path)
            .map_err(|error| AppError::State(format!("无法移除旧版本链接：{error}"))),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AppError::State(format!("无法读取旧版本链接：{error}"))),
    }
}

fn systemd_unit_is_loaded(output: &str, unit_name: &str) -> bool {
    output.split("\n\n").any(|section| {
        let mut id_matches = false;
        let mut loaded = false;
        for line in section.lines() {
            if line == format!("Id={unit_name}") {
                id_matches = true;
            } else if line == "LoadState=loaded" {
                loaded = true;
            }
        }
        id_matches && loaded
    })
}

fn set_mode(path: &Path, mode: u32, message: &str) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        fs::set_permissions(path, fs::Permissions::from_mode(mode))
            .map_err(|error| AppError::Permission(format!("{message}：{error}")))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, mode, message);
        Ok(())
    }
}

fn create_directory_symlink(source: &Path, destination: &Path) -> Result<(), AppError> {
    #[cfg(unix)]
    {
        symlink(source, destination)
            .map_err(|error| AppError::State(format!("无法创建当前版本链接：{error}")))
    }
    #[cfg(not(unix))]
    {
        let _ = (source, destination);
        Err(AppError::Service("当前平台不支持 Linux 版本链接".into()))
    }
}
