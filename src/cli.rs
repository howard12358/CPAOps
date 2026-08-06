use std::path::PathBuf;

use clap::{Parser, Subcommand};

#[derive(Debug, Parser)]
#[command(name = "cpactl", version, about = "CPA Stack 跨平台运维工具")]
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
