use clap::Parser;
use cpactl::app::App;
use cpactl::cli::Cli;
use cpactl::domain::error::AppError;
use cpactl::domain::runtime::RuntimePaths;
use cpactl::output::Output;
use cpactl::platform::native_platform;
use std::thread;
use std::time::Duration;

fn main() {
    let cli = Cli::parse();
    let result = (|| {
        let paths = RuntimePaths::resolve(cli.root.clone())?;
        let platform = native_platform(paths.clone())?;
        let app = App::new(paths, platform);
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
        println!("{}", output.message);
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
