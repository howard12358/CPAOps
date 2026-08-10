# CPA Stack CLI

`cpactl` 是 CPA（CLIProxyAPI）和 cpa-usage-keeper 的跨平台运维命令行工具。它从 GitHub Release 下载对应平台资产，校验 SHA-256、原子激活版本，并在更新失败时恢复受影响服务的原版本和运行状态。

支持 macOS Apple Silicon 和 Windows x64。

## 安装 cpactl

macOS：

```sh
curl -fsSL https://raw.githubusercontent.com/howard12358/CPAOps/main/scripts/install.sh | bash
```

Windows（以管理员身份打开 PowerShell）：

```powershell
irm https://raw.githubusercontent.com/howard12358/CPAOps/main/scripts/install.ps1 | iex
```

安装脚本会校验下载文件，并将 `cpactl` 加入当前用户的全局命令路径。支持常见代理环境变量，例如：

```sh
export https_proxy=http://127.0.0.1:7897
export http_proxy=http://127.0.0.1:7897
export all_proxy=socks5://127.0.0.1:7897
```

Windows PowerShell 也可以直接粘贴代理软件导出的 `$env:HTTP_PROXY=...` 形式。

## 首次安装服务

```sh
cpactl install
```

首次执行会提示输入 CPA 管理密钥和 Keeper 登录密码；未保存下载代理时，也会询问是否配置。Windows 必须使用提升权限的管理员 PowerShell。

GitHub 下载因 401 或 403 被拒绝时，使用浏览器完成认证：

```sh
cpactl auth login
cpactl auth status
cpactl auth logout
```

## 常用命令

```sh
cpactl status                         # 查看服务、端口和版本
cpactl status --json                  # 输出 JSON
cpactl start [cli|keeper]             # 启动一个或全部服务
cpactl stop [cli|keeper]              # 停止服务并阻止自动拉起
cpactl restart [cli|keeper]           # 重启服务
cpactl logs cli -f                    # 跟随 CLIProxyAPI 日志
cpactl logs keeper -n 200             # 查看 Keeper 最后 200 行日志
cpactl update [cli|keeper]            # 更新服务 Release
cpactl rollback keeper --version v1.14.3
cpactl upgrade                         # 更新 cpactl 自身
cpactl proxy set                      # 保存下载代理
cpactl proxy show
cpactl cache clean                    # 清理可重新下载的缓存
cpactl cache clean --dry-run          # 预览可释放空间，不删除文件
cpactl path --open                    # 在 Finder / Explorer 打开运行目录
cpactl path --shell                   # 输出可直接粘贴的 cd 命令
cpactl -V                             # 版本、提交和构建时间
```

`install`、`update` 和 `upgrade` 在交互式终端中显示下载进度；`--json`、管道和重定向输出不会包含动画控制字符。

`cache clean` 只删除 `downloads/` 中的下载包、校验文件和 Release 元数据；不会触碰配置、授权、Keeper 数据库、历史 Release 或日志。在交互式终端中，它会显示已删除项目数和已释放空间。

## 运行目录与卸载

默认运行目录：

- macOS：`~/Library/Application Support/cpa-stack`
- Windows：`C:\ProgramData\CPAStack`

可通过 `--root <path>` 或 `CPA_STACK_ROOT` 覆盖，`--root` 优先。测试时请使用单独的临时根目录，避免影响正在使用的服务和数据。

```sh
cpactl uninstall          # 移除服务定义，保留运行数据
cpactl uninstall --purge  # 输入 DELETE 后删除整个运行目录
```

`--purge` 会删除服务配置、授权文件、Keeper 数据库、Release、日志和下载缓存。Windows 的 GitHub Token 与代理位于 `%LOCALAPPDATA%\CPAStack\config`，不会被该命令删除；macOS 默认位置与运行目录相同，因此会一并删除。

## 开发

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo build --release
```
