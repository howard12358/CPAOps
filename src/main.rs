use clap::Parser;
use cpactl::app::App;
use cpactl::cli::Cli;
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::output::Output;
use cpactl::platform::native_platform;
use cpactl::progress::{NoProgress, ProgressReporter, TerminalProgress};
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
        let platform = native_platform(paths.clone())?;
        let progress: Arc<dyn ProgressReporter> = if !cli.json && std::io::stderr().is_terminal() {
            Arc::new(TerminalProgress::new())
        } else {
            Arc::new(NoProgress)
        };
        let app = App::new(paths, platform).with_progress(progress);
        let output = app.run(&cli.command)?;
        print_output(&output, cli.json);
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
        print_failure(&error, cli.json);
        std::process::exit(i32::from(error.exit_code()));
    }
}

fn print_output(output: &Output, json: bool) {
    if json {
        println!("{}", output.to_json());
    } else {
        println!("{}", output.human_message());
    }
}

fn print_failure(error: &AppError, json: bool) {
    let output = Output::failure(error.exit_code(), error.to_string());
    if json {
        println!("{}", output.to_json());
    } else {
        eprintln!("错误：{}", output.message);
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
