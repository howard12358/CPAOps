# CPA Local Deploy

在 macOS Apple Silicon 上以当前用户的 LaunchAgent 方式运行 CLIProxyAPI（CPA）和 cpa-usage-keeper。

## 安装

```sh
sh macos/install.sh --init
```

也可先复制 `config/` 中两个 `.example` 文件到 `~/Library/Application Support/cpa-stack/config/` 并填好必填项（权限为 `600`），再运行 `sh macos/install.sh`。

脚本只支持 Apple Silicon，自动从官方 GitHub Release 下载并校验 SHA-256。服务仅绑定本机的 `127.0.0.1:8317` 和 `127.0.0.1:18080`。下载默认匿名访问；仅在 GitHub 返回 `401` 或 `403` 时才交互要求 token，并将其以 `600` 权限保存到运行目录的 `config/github-token`。后续 token 失效时会要求输入新 token 并替换旧值。

## 运维

```sh
sh macos/start.sh [cli|keeper]
sh macos/stop.sh [cli|keeper]
sh macos/restart.sh [cli|keeper]
sh macos/status.sh
sh macos/update.sh [cli|keeper]
sh macos/uninstall.sh
sh macos/uninstall.sh --purge
```

`stop.sh` 仅停止当前服务，保留登录自启注册；`uninstall.sh` 才注销 LaunchAgent。`--purge` 需要输入 `DELETE`，会删除整个运行目录。

## 迁移与安全

迁移旧环境时，先停止服务，再手工复制 `auths/` 与 Keeper SQLite 文件到运行目录。不要提交、分享或同步运行目录中的配置、认证文件、数据库、日志和下载包。

## 验证

```sh
sh tests/run.sh
find macos tests -name '*.sh' -exec sh -n {} +
```
