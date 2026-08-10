use std::io::{self, IsTerminal};

use serde_json::json;

use crate::cli::AuthAction;
use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::github::GithubClient;
use crate::output::Output;
use crate::storage::config::{ConfigStore, GithubTokenStore};

pub fn run(paths: RuntimePaths, action: &AuthAction) -> Result<Output, AppError> {
    let config = ConfigStore::new(paths);
    let token_store = GithubTokenStore::default_location();
    match action {
        AuthAction::Login { token } => login(&config, &token_store, *token),
        AuthAction::Status => status(&token_store),
        AuthAction::Logout => {
            token_store.clear()?;
            Ok(Output::success("已退出 GitHub 认证"))
        }
    }
}

fn login(
    config: &ConfigStore,
    token_store: &GithubTokenStore,
    use_manual_token: bool,
) -> Result<Output, AppError> {
    if !io::stdin().is_terminal() {
        return Err(AppError::Usage(
            "GitHub 登录仅允许在交互式终端中执行".into(),
        ));
    }
    let token = if use_manual_token {
        eprint!("请输入 GitHub Personal Access Token：");
        io::Write::flush(&mut io::stderr())
            .map_err(|_| AppError::Internal("无法写入 GitHub Token 提示".into()))?;
        rpassword::read_password()
            .map_err(|_| AppError::Internal("无法读取 GitHub Token".into()))?
    } else {
        GithubClient::new(config.clone())?.device_login()?
    };
    token_store.save(&token)?;
    Ok(Output::success("GitHub 认证已保存"))
}

fn status(token_store: &GithubTokenStore) -> Result<Output, AppError> {
    let authenticated = token_store.load()?.is_some();
    Ok(Output::success_with_data(
        if authenticated {
            "GitHub 已认证（Token 已保存）"
        } else {
            "GitHub 未认证"
        },
        json!({ "authenticated": authenticated, "token_path": token_store.path().display().to_string() }),
    ))
}
