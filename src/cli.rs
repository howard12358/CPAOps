use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(
    name = "cpactl",
    version,
    about = "CPA Stack 跨平台运维工具",
    after_help = "示例：\n  cpactl start keeper\n  cpactl logs cli -f\n  cpactl rollback keeper --version v1.14.3\n  cpactl --json status"
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
    #[command(about = "安装或修复服务")]
    Install,
    #[command(about = "启动服务；省略服务名时启动全部")]
    Start { service: Option<String> },
    #[command(about = "停止服务并阻止自动拉起")]
    Stop { service: Option<String> },
    #[command(about = "清除停用标记后重启服务")]
    Restart { service: Option<String> },
    #[command(about = "查看注册、监听端口和当前版本")]
    Status,
    #[command(about = "查看或跟随服务日志")]
    Logs {
        service: String,
        #[arg(short = 'f')]
        follow: bool,
        #[arg(short = 'n', default_value_t = 200)]
        lines: usize,
    },
    #[command(about = "查询并更新到 GitHub 最新 Release")]
    Update { service: Option<String> },
    #[command(about = "检查或更新 cpactl 自身")]
    Upgrade {
        #[arg(long)]
        check: bool,
    },
    #[command(about = "切换到本机已验证的历史版本")]
    Rollback {
        service: String,
        #[arg(long)]
        version: String,
    },
    #[command(about = "保存、查看或清除下载代理")]
    Proxy {
        #[command(subcommand)]
        action: ProxyAction,
    },
    #[command(about = "登录、查看或退出 GitHub 认证")]
    Auth {
        #[command(subcommand)]
        action: AuthAction,
    },
    #[command(about = "输出运行目录；--open 打开目录，--shell 输出可粘贴的跳转命令")]
    Path {
        #[arg(long, conflicts_with = "shell")]
        open: bool,
        #[arg(long)]
        shell: bool,
    },
    #[command(about = "移除服务定义；--purge 删除运行数据")]
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

#[derive(Debug, Subcommand)]
pub enum AuthAction {
    #[command(about = "通过 GitHub Device Flow 登录；--token 手工输入 PAT")]
    Login {
        #[arg(long)]
        token: bool,
    },
    #[command(about = "查看 GitHub 认证状态")]
    Status,
    #[command(about = "清除本地 GitHub 认证")]
    Logout,
}
