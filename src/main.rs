mod cli;
mod domain;
mod output;

use clap::Parser;
use cli::Cli;
use domain::error::AppError;
use output::Output;

fn main() {
    let cli = Cli::parse();
    let result: Result<(), AppError> = match cli.command {
        cli::Command::Status => Err(AppError::State(
            "尚未安装 CPA Stack，请先运行 cpactl install".into(),
        )),
        _ => Err(AppError::State("该命令尚未实现".into())),
    };

    if let Err(error) = result {
        print_failure(&error, cli.json);
        std::process::exit(i32::from(error.exit_code()));
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
