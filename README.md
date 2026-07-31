# CPA 与 usage-keeper 本地部署

在 macOS Apple Silicon（M 系列芯片）上，以当前用户的 `LaunchAgent` 运行：

- [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI)（下文简称 CPA）
- [cpa-usage-keeper](https://github.com/Willxup/cpa-usage-keeper)（下文简称 Keeper）

仓库只保存部署脚本和配置模板；服务二进制、认证文件、数据库、日志、代理和 GitHub token 都放在独立的私有运行目录，删除仓库不会删除业务数据。

Windows x64 裸机部署说明见 [windows/README.md](windows/README.md)。

## 支持范围与前置条件

- macOS Apple Silicon：`uname -s` 必须为 `Darwin`，`uname -m` 必须为 `arm64`。
- 使用系统自带的 `sh`、`curl`、`tar`、`shasum`、`plutil`、`launchctl`；不需要 Homebrew、Docker、sudo 或 zsh。
- 需要网络访问 GitHub。若网络受限，可在首次安装时粘贴代理软件导出的代理环境变量。

服务默认端口：CPA 为 `8317`，Keeper 为 `18080`。安装前请确认这两个端口没有被其他进程占用。

> CPA 会绑定到 `127.0.0.1:8317`。Keeper 当前版本实际监听 `*:18080`；如需隔离局域网访问，请在主机防火墙或反向代理层限制该端口。

## 快速开始

```sh
git clone <本仓库地址>
cd CPAOps/macos
./install.sh
```

首次运行时脚本会：

1. 创建私有运行目录；
2. 隐藏输入 CPA 管理密钥与 Keeper 登录密码；
3. 检查当前 shell 或已保存的代理；没有代理时可以直接回车跳过，或粘贴代理配置；
4. 从官方 GitHub Release 下载 CPA 与 Keeper 的 Apple Silicon 二进制，并用 `checksums.txt` 进行 SHA-256 校验；
5. 写入当前用户的 LaunchAgent 并启动两个服务。

安装完成后检查状态：

```sh
./status.sh
```

预期会显示两项服务的加载状态、当前版本和端口监听信息。

## 首次配置说明

### CPA 管理密钥

安装器会要求输入 CPA 管理密钥，并写入 `config/config.yaml`。Keeper 使用同一个密钥访问 CPA 管理接口。

### Keeper 登录密码

安装器会要求输入 Keeper 登录密码，写入 `config/keeper.env`。默认启用 Keeper 登录保护。

### 代理

没有发现代理时，可粘贴多数代理软件直接导出的格式：

```sh
export https_proxy=http://127.0.0.1:7897 http_proxy=http://127.0.0.1:7897 all_proxy=socks5://127.0.0.1:7897
```

直接回车表示本次不配置代理。保存后的代理会被安装和更新脚本自动加载。

## 日常运维

所有命令均在 `macos/` 目录执行。

| 操作 | 命令 |
| --- | --- |
| 查看状态 | `./status.sh` |
| 启动全部服务 | `./start.sh` |
| 启动 CPA / Keeper | `./start.sh cli` / `./start.sh keeper` |
| 停止全部服务 | `./stop.sh` |
| 停止 CPA / Keeper | `./stop.sh cli` / `./stop.sh keeper` |
| 重启全部服务 | `./restart.sh` |
| 更新全部服务 | `./update.sh` |
| 更新 CPA / Keeper | `./update.sh cli` / `./update.sh keeper` |
| 查看 CPA / Keeper 日志 | `./logs.sh cli` / `./logs.sh keeper` |
| 输出运行目录绝对路径 | `./stack-path.sh` |
| 设置代理 | `./proxy.sh set` |
| 查看代理是否已配置 | `./proxy.sh show` |
| 移除保存的代理 | `./proxy.sh clear` |

`stop.sh` 会停止服务并写入停用标记，因此服务不会被 `KeepAlive` 立即拉起；它不会取消登录自启。再次执行 `start.sh` 会清除停用标记。

## 更新机制

`update.sh` 对 CPA、Keeper 分别执行以下步骤：

1. 获取官方 GitHub Release 的最新版本信息；
2. 先匿名访问 GitHub；只有收到 `401` 或 `403` 时才使用已保存 token，必要时交互要求新 token；
3. 下载 Apple Silicon 对应压缩包和 `checksums.txt`；
4. 校验 SHA-256，失败时不切换当前版本；
5. 解压、验证二进制可运行、原子切换 `current/` 软链接；
6. 重启对应 LaunchAgent。

更新 Keeper 前会尝试在 `keeper/` 下创建 SQLite 备份。

## 运行目录

所有运行态文件位于：

```text
~/Library/Application Support/cpa-stack/
├── config/
│   ├── config.yaml          # CPA 配置
│   ├── keeper.env           # Keeper 配置
│   ├── proxy.env            # 保存的代理
│   └── github-token         # GitHub token（仅在需要时保存）
├── auths/                   # CPA OAuth / auth 文件
├── keeper/
│   ├── app.db               # Keeper SQLite 数据库
│   └── migration-backups/   # 数据迁移前备份
├── releases/                # 已下载的服务版本
├── current/                 # 当前启用版本的软链接
├── logs/                    # LaunchAgent stdout/stderr 日志
└── state/                   # 手动停用标记
```

私密配置和 token 权限为 `600`；认证、数据库和日志目录不应提交、分享或同步到公共位置。

## 从旧环境迁移

如果原环境位于：

```text
~/Library/Application Support/cliproxyapi-llh/
```

迁移前应停止新 Keeper，然后：

- 将旧 `auths/` 中的认证文件复制到新运行目录的 `auths/`；
- 使用 `sqlite3 .backup` 对旧 `keeper/app.db` 生成一致性备份，再替换新 `keeper/app.db`；
- 启动 Keeper 并用 `./status.sh` 检查端口和日志。

迁移不会自动删除旧目录。确认新服务稳定后，再自行归档或移除旧环境。

## 卸载

只取消登录自启、保留所有运行数据：

```sh
./uninstall.sh
```

注销服务并删除整个运行目录：

```sh
./uninstall.sh --purge
```

`--purge` 会要求输入 `DELETE`，并会删除配置、认证、数据库、日志、下载版本和备份；确认不再需要数据前不要执行。

## 故障排查

```sh
# 查看服务状态与监听端口
./status.sh

# 查看 CPA 日志
tail -n 200 -f "$HOME/Library/Application Support/cpa-stack/logs/cli-proxy-api.out.log" \
  "$HOME/Library/Application Support/cpa-stack/logs/cli-proxy-api.err.log"

# 查看 Keeper 日志
tail -n 200 -f "$HOME/Library/Application Support/cpa-stack/logs/cpa-usage-keeper.out.log" \
  "$HOME/Library/Application Support/cpa-stack/logs/cpa-usage-keeper.err.log"
```

若安装/更新无法访问 GitHub，先执行 `./proxy.sh show` 确认代理状态；若 GitHub 返回访问限制，安装器会自动使用已保存 token 或提示输入新 token。

## 开发验证

```sh
sh tests/run.sh
find macos tests -name '*.sh' -exec sh -n {} +
```
