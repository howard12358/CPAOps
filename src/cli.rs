use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cpactl",
    version,
    about = "CPA Stack 跨平台运维工具",
    after_help = "命令说明：\n  install    安装或修复服务，并下载验证最新 Release\n  start      启动一个或全部服务\n  stop       停止服务并阻止 LaunchAgent 自动拉起\n  restart    清除停用标记后重启服务\n  status     查看注册、监听端口和当前版本\n  logs       查看或跟随服务日志\n  update     查询并更新到 GitHub 最新 Release\n  rollback   切换到本机已验证的历史版本\n  proxy      保存、查看或清除下载代理\n  path       输出当前运行目录\n  uninstall  移除服务定义，默认保留运行数据\n\n示例：\n  cpactl start keeper\n  cpactl logs cli -f\n  cpactl rollback keeper --version v1.14.3\n  cpactl --json status"
)]
pub struct Cli {
    #[arg(long, global = true)]
    pub root: Option<PathBuf>,
    #[arg(long, global = true)]
    pub json: bool,
    #[command(subcommand)]
    pub command: Command,
}

#[derive(Debug, Subcommand)]
pub enum Command {
    Install,
    Start {
        service: Option<String>,
    },
    Stop {
        service: Option<String>,
    },
    Restart {
        service: Option<String>,
    },
    Status,
    Logs {
        service: String,
        #[arg(short = 'f')]
        follow: bool,
        #[arg(short = 'n', default_value_t = 200)]
        lines: usize,
    },
    Update {
        service: Option<String>,
    },
    Rollback {
        service: String,
        #[arg(long)]
        version: String,
    },
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },
    Path,
    Uninstall {
        #[arg(long)]
        purge: bool,
    },
}

#[derive(Debug, Subcommand)]
pub enum ProxyAction {
    Set,
    Show,
    Clear,
}
