use clap::Parser;
use cpactl::app::App;
use cpactl::cli::{Cli, Command};
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::output::Output;
use cpactl::platform::native_platform;
use cpactl::progress::{NoProgress, ProgressReporter, TerminalProgress};
use std::process::Command as ProcessCommand;
use std::thread;
use std::time::Duration;
use std::{io::IsTerminal, sync::Arc};

fn main() {
    let arguments = std::env::args_os().collect::<Vec<_>>();
    if arguments.len() == 2 && matches!(arguments[1].to_str(), Some("-V" | "--version")) {
        println!("{}", cpactl::build_info::version_text());
        return;
    }
    if arguments.len() == 2 && matches!(arguments[1].to_str(), Some("--build-info")) {
        println!("{}", cpactl::build_info::build_info_text());
        return;
    }
    let cli = Cli::parse();
    let result = (|| {
        let paths = RuntimePaths::resolve(cli.root.clone())?;
        let interactive =
            !cli.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
        let progress: Arc<dyn ProgressReporter> = if interactive {
            Arc::new(TerminalProgress::new())
        } else {
            Arc::new(NoProgress)
        };
        if let Command::Auth { action } = &cli.command {
            let output = cpactl::auth::run(paths, action)?;
            print_output(&output, cli.json, cli.debug);
            return Ok(());
        }
        if let Command::Upgrade { check } = &cli.command {
            let output = cpactl::upgrade::run(paths, *check, progress.as_ref())?;
            print_output(&output, cli.json, cli.debug);
            return Ok(());
        }
        let platform = native_platform(paths.clone())?;
        let runtime_root = paths.root.clone();
        let app = App::new(paths, platform)
            .with_progress(progress)
            .with_interactive_proxy_prompt(interactive);
        let output = app.run(&cli.command)?;
        if let Command::Path { open: true, .. } = &cli.command {
            open_directory(&runtime_root)?;
        }
        print_output(&output, cli.json, cli.debug);
        if !output.ok {
            std::process::exit(i32::from(output.code));
        }

        if let cpactl::cli::Command::Logs {
            service,
            follow: true,
            ..
        } = &cli.command
        {
            follow_logs(app.log_follower(service)?)?;
        }
        Ok(())
    })();

    if let Err(error) = result {
        print_failure(&error, cli.json, cli.debug);
        std::process::exit(i32::from(error.exit_code()));
    }
}

fn open_directory(path: &std::path::Path) -> Result<(), AppError> {
    #[cfg(target_os = "macos")]
    let result = ProcessCommand::new("open").arg(path).status();
    #[cfg(target_os = "windows")]
    let result = ProcessCommand::new("explorer").arg(path).status();
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = ProcessCommand::new("xdg-open").arg(path).status();

    result
        .map_err(|_| AppError::State("无法打开运行目录".into()))?
        .success()
        .then_some(())
        .ok_or_else(|| AppError::State("无法打开运行目录".into()))
}

fn print_output(output: &Output, json: bool, debug: bool) {
    if json {
        println!("{}", output.to_json_with_debug(debug));
    } else {
        println!("{}", output.human_message());
        if debug {
            if let Some(debug_text) = output.debug_text() {
                eprintln!("原始诊断：\n{debug_text}");
            }
        }
    }
}

fn print_failure(error: &AppError, json: bool, debug: bool) {
    let output = Output::from_error(error, debug);
    if json {
        println!("{}", output.to_json());
    } else {
        eprintln!("错误：{}", output.message);
        if debug {
            if let Some(raw_diagnostic) = error.raw_diagnostic() {
                eprintln!("原始诊断：\n{raw_diagnostic}");
            }
        }
    }
}

fn follow_logs(mut follower: cpactl::app::LogFollower) -> Result<(), AppError> {
    loop {
        for line in follower.poll()? {
            println!("{line}");
        }
        thread::sleep(Duration::from_millis(250));
    }
}
