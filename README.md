# CPA Stack 本地部署

`cpactl` 是 CPA（CLIProxyAPI）和 cpa-usage-keeper 的跨平台运维工具。它从官方 GitHub Release 下载对应平台资产，校验 SHA-256 后原子激活版本；升级失败时自动恢复该服务原有版本和运行状态。

支持 macOS Apple Silicon（当前用户 LaunchAgent）和 Windows x64（SYSTEM 计划任务）。完整命令、权限、迁移与故障处理见 [cpactl 使用说明](docs/cpactl.md)。

## 快速开始

先从本仓库构建或安装 `cpactl`，然后在首次安装前提供两个私密配置值：

```sh
export CPA_MANAGEMENT_KEY='请使用自己的管理密钥'
export KEEPER_LOGIN_PASSWORD='请使用自己的 Keeper 登录密码'
cpactl install
cpactl status
```

在交互式终端运行 `cpactl install` 时也会隐藏输入这两个值。Windows 必须在提升权限的管理员 PowerShell 中运行；macOS 不需要 `sudo`。

日常命令示例：

```sh
cpactl status --json
cpactl logs cli -f
cpactl update
cpactl rollback keeper --version v1.2.3
cpactl proxy set                 # 从 HTTP_PROXY/HTTPS_PROXY/ALL_PROXY 保存代理
cpactl uninstall                 # 保留运行数据
```

默认运行目录为 macOS 的 `~/Library/Application Support/cpa-stack` 和 Windows 的 `C:\ProgramData\CPAStack`。可用 `--root <path>` 或 `CPA_STACK_ROOT` 覆盖，前者优先。

## 安全与更新

`cpactl` 只激活带有 `.verified` 标记的本地版本：发布资产和 `checksums.txt` 必须精确匹配，二进制须可通过启动验证。更新全部服务时逐项处理；一个服务失败只回退该服务，不会降级已经更新成功的其他服务。Keeper 更新前会备份 SQLite 数据库及 WAL/SHM 文件。

配置、认证、Token、代理、数据库、下载与日志都保存在私有运行目录中，不会写入仓库，也不会出现在 JSON 或错误输出中。

## 旧脚本兼容层

`macos/` 下的 Shell 脚本与 `windows/` 下的 PowerShell 脚本目前仍保留，作为真实机器验收完成前的兼容层。新部署和日常操作请使用 `cpactl`；旧脚本不再是新增功能的接口。

## 开发验证

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
sh tests/run.sh
```
