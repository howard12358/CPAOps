use std::io::{self, IsTerminal};

use serde_json::json;

use crate::cli::AuthAction;
use crate::domain::error::AppError;
use crate::domain::runtime::RuntimePaths;
use crate::github::GithubClient;
use crate::output::Output;
use crate::storage::config::ConfigStore;

pub fn run(paths: RuntimePaths, action: &AuthAction) -> Result<Output, AppError> {
    let config = ConfigStore::new(paths);
    match action {
        AuthAction::Login { token } => login(&config, *token),
        AuthAction::Status => status(&config),
        AuthAction::Logout => {
            config.clear_token()?;
            Ok(Output::success("已退出 GitHub 认证"))
        }
    }
}

fn login(config: &ConfigStore, use_manual_token: bool) -> Result<Output, AppError> {
    if !io::stdin().is_terminal() {
        return Err(AppError::Usage(
            "GitHub 登录仅允许在交互式终端中执行".into(),
        ));
    }
    let token = if use_manual_token {
        rpassword::prompt_password("请输入 GitHub Personal Access Token：")
            .map_err(|_| AppError::Internal("无法读取 GitHub Token".into()))?
    } else {
        GithubClient::new(config.clone())?.device_login()?
    };
    config.save_token(&token)?;
    Ok(Output::success("GitHub 认证已保存"))
}

fn status(config: &ConfigStore) -> Result<Output, AppError> {
    let authenticated = config.load_token()?.is_some();
    Ok(Output::success_with_data(
        if authenticated {
            "GitHub 已认证"
        } else {
            "GitHub 未认证"
        },
        json!({ "authenticated": authenticated }),
    ))
}
