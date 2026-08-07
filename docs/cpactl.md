# cpactl 使用说明

## 支持平台与权限

| 平台 | 架构 | 默认根目录 | 服务托管 | 权限 |
| --- | --- | --- | --- | --- |
| macOS | Apple Silicon (`aarch64`) | `~/Library/Application Support/cpa-stack` | 当前用户 LaunchAgent | 不需要 `sudo` |
| Windows | x64 | `C:\ProgramData\CPAStack` | SYSTEM 开机计划任务 | 管理员 PowerShell |

只支持上述平台组合。`--root <path>` 覆盖 `CPA_STACK_ROOT`；两者都不设置时使用平台默认目录。运行目录内有 `config/`、`auths/`、`keeper/`、`releases/`、`current/`、`downloads/`、`logs/`、`state/`、`bin/` 和 `tasks/`。

## 安装

首次安装需要 CPA 管理密钥与 Keeper 登录密码。优先从环境变量读取，避免出现在命令行历史中：

```sh
export CPA_MANAGEMENT_KEY='...'
export KEEPER_LOGIN_PASSWORD='...'
cpactl install
```

交互式终端会隐藏输入；非交互环境必须设置以上变量。已有配置不会被覆盖，但安装和更新都会拒绝含 `__REQUIRED__` 占位符、无效端口或权限不安全的配置。

安装会创建私有布局、下载并校验两个服务、注册平台服务，并激活通过验证的版本。Release 资产必须有唯一的当前平台压缩包和 `checksums.txt`；解压后的预期二进制还会以 `--help` 启动验证。

## 命令

```text
cpactl install
cpactl start [cli|keeper]
cpactl stop [cli|keeper]
cpactl restart [cli|keeper]
cpactl status [--json]
cpactl logs <cli|keeper> [-f] [-n 200]
cpactl update [cli|keeper]
cpactl upgrade [--check]
cpactl rollback <cli|keeper> --version <version>
cpactl proxy set|show|clear
cpactl auth login|status|logout
cpactl path [--open|--shell]
cpactl uninstall [--purge]
```

`cpactl update` 只更新 CPA 和 Keeper；`cpactl upgrade` 只更新运维 CLI 本身。两者都会验证对应 GitHub Release 的 SHA-256。`upgrade --check` 只报告可用的新版本。

`cli` 与 `cli-proxy-api` 等价；`keeper` 与 `cpa-usage-keeper` 等价。不带服务参数的启动、停止、重启和更新按顺序处理两个服务。

`stop` 会写入停用标记，避免托管器立即拉起服务；`start` 和 `restart` 会清除它。`logs` 读取统一的 `logs/<service>.out.log` 与 `logs/<service>.err.log`。

## 更新、回滚与代理

更新先匿名访问 GitHub，只有收到 401/403 后才使用已保存的 Token。代理只保存为运行目录内的结构化配置，仅注入当前 `cpactl` 下载请求，不修改系统代理：

```sh
export HTTPS_PROXY=http://127.0.0.1:7897
export ALL_PROXY=socks5://127.0.0.1:7897
cpactl proxy set
cpactl update keeper
```

更新全部服务时会逐项输出结果；失败服务自动切回此前版本和运行状态，成功服务保持新版本。Keeper 更新前会在 `keeper/migration-backups/` 备份 `app.db`、`app.db-wal` 和 `app.db-shm`。

回滚只接受本机 `releases/<service>/<version>/` 中已经验证且拥有预期二进制的版本：

```sh
cpactl rollback cli --version v1.2.3
```

不存在、未验证或路径不安全的版本均会被拒绝。

## JSON 与退出码

加 `--json` 输出稳定的 `ok`、`code`、`message` 和非敏感 `data` 字段。更新多个服务时 `data.services` 逐项报告成功或失败；发生部分失败时命令以失败退出码结束，但其中成功服务不会被回退。

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 2 | 参数或用法错误 |
| 3 | 权限不足 |
| 4 | 未安装或状态不允许 |
| 5 | 网络、代理或 GitHub 访问失败 |
| 6 | 资产、校验和或解压验证失败 |
| 7 | 服务、端口健康检查或回滚失败 |
| 1 | 其他内部错误 |

输出不会包含管理密钥、Keeper 密码、GitHub Token 或代理 URL。

## 迁移、卸载与故障处理

旧安装目录不会自动迁移。迁移时先停止新 Keeper，再复制旧 `auths/`；对 Keeper 数据库使用 SQLite 的 `.backup` 生成一致备份，然后写入新根目录的 `keeper/app.db`。启动后运行 `cpactl status` 和 `cpactl logs keeper -f` 验证。

`cpactl uninstall` 只移除 LaunchAgent 或计划任务并保留数据。`cpactl uninstall --purge` 仅在交互式终端接受精确的 `DELETE` 确认，且会在路径边界校验后删除运行目录。

无法访问 GitHub 时，先执行 `cpactl proxy show`；收到访问限制时配置 GitHub Token。服务无法启动时查看 `cpactl logs <service> -n 200`，确认端口 8317（CPA）或 18080（Keeper）没有冲突。

## 旧脚本兼容层

仓库现有 macOS Shell 和 Windows PowerShell 脚本保留为兼容层，直到 macOS Apple Silicon 与 Windows x64 的真实机器验收完成。它们不应作为新自动化或新功能的入口；请使用本文档中的 `cpactl` 命令。
