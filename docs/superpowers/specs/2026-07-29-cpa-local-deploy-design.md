# CPA 与 usage-keeper 本地部署设计（macOS Apple Silicon）

## 目标与范围

建立一个可直接克隆的部署仓库，使用户在 macOS Apple Silicon 上进入 `macos/` 目录并运行 POSIX `sh` 脚本，即可：

- 准备 CPA（CLIProxyAPI）与 cpa-usage-keeper 的私有运行环境；
- 从各自官方 GitHub Release 下载、校验并安装 macOS ARM64 二进制；
- 将两项服务注册为当前用户登录时自动启动的 LaunchAgent；
- 提供启动、停止、重启、状态、升级、回滚和卸载操作；
- 在升级失败时保持当前已运行版本和业务数据不变。

首版只支持 macOS Apple Silicon（`Darwin` + `arm64`）。不支持 Intel macOS、Linux、Windows、Docker、局域网/EasyTier 暴露和 PF 防火墙规则配置。

## 已知运行模型

- CPA 监听 `127.0.0.1:8317`。
- Keeper 监听 `127.0.0.1:18080`，通过 `http://127.0.0.1:8317` 访问 CPA。
- CPA 使用 `config.yaml` 和 `auths/`；Keeper 使用环境文件和 SQLite 数据目录。
- 两者均由用户级 LaunchAgent 运行，采用 `RunAtLoad`、`KeepAlive` 与 `ThrottleInterval=10`。

现有安装中的 API key、管理密钥、代理地址、认证文件、SQLite 数据库、日志和下载包均为敏感或运行态数据。本仓库不得复制、提交或自动迁移这些内容。

## 选定架构

采用“仓库部署器 + 独立运行目录”：Git 仓库只包含脚本、模板、LaunchAgent 模板、测试和文档；运行数据不位于仓库内。

相较于把运行数据放在仓库中，该设计避免 `git pull`、删除克隆目录或误提交密钥影响业务数据。相较于 Docker Compose，它不需要 Docker Desktop，并延续原生 `launchd` 的登录自启模型。

### 仓库结构

```text
cpa-local-deploy/
├── README.md
├── .gitignore
├── config/
│   ├── cpa.config.yaml.example
│   └── keeper.env.example
├── macos/
│   ├── install.sh
│   ├── start.sh
│   ├── stop.sh
│   ├── restart.sh
│   ├── status.sh
│   ├── update.sh
│   ├── uninstall.sh
│   ├── lib/
│   │   ├── common.sh
│   │   ├── config.sh
│   │   ├── releases.sh
│   │   └── launchd.sh
│   └── launchd/
│       ├── io.cpa-local.cli-proxy-api.plist.in
│       └── io.cpa-local.usage-keeper.plist.in
├── tests/
└── docs/superpowers/specs/
```

### 私有运行目录

安装根目录固定为：`$HOME/Library/Application Support/cpa-stack`。

```text
cpa-stack/
├── config/
│   ├── config.yaml
│   └── keeper.env
├── auths/
├── keeper/
├── releases/
│   ├── cli-proxy-api/<version>/
│   └── cpa-usage-keeper/<version>/
├── current/
│   ├── cli-proxy-api -> ../releases/cli-proxy-api/<version>
│   └── cpa-usage-keeper -> ../releases/cpa-usage-keeper/<version>
├── downloads/
├── logs/
└── state/
    ├── cli.disabled
    └── keeper.disabled
```

`config/` 下的私密文件权限为 `600`；`auths/`、`keeper/` 和 `state/` 为 `700`。安装过程使用 `umask 077`。

`current/` 是唯一被 LaunchAgent 引用的二进制入口。升级时先把新版本完整下载、校验、解包和自检到 `releases/`，再用临时符号链接和 `mv` 原子切换。下载或校验失败绝不修改 `current/`。

## 配置与首次安装

提供两条等价路径。

### 交互初始化

```sh
sh macos/install.sh --init
```

安装器会：

1. 验证操作系统、CPU 架构与必要工具：`curl`、`tar`、`shasum`、`plutil`、`launchctl`。
2. 创建运行目录，但不会覆盖已有配置、认证文件或数据库。
3. 不回显地读取 CPA 管理密钥；询问 Keeper 是否开启登录保护，若开启则不回显地读取密码；可选读取 CPA 上游代理 URL。
4. 从模板生成配置，并验证所有必填项已替换、端口合法且配置文件权限正确。
5. 下载、校验和安装两项服务；渲染 plist；注册并启动 LaunchAgent；执行健康检查。

### 预先编辑模板

```sh
cp config/cpa.config.yaml.example \
  "$HOME/Library/Application Support/cpa-stack/config/config.yaml"
cp config/keeper.env.example \
  "$HOME/Library/Application Support/cpa-stack/config/keeper.env"
# 用户填写必填值后：
sh macos/install.sh
```

模板中的必填项使用明确的 `__REQUIRED__` 占位符。安装器拒绝未替换的占位符、无效配置或权限过宽的私密文件。

首版不自动迁移旧安装。README 提供手工迁移步骤：在服务停止后复制旧的 `auths/` 和 Keeper SQLite 数据到运行目录，确认权限后再启动；旧密钥和配置由用户有意识地重建或迁移。

## 发布下载与校验

- CPA 从 `router-for-me/CLIProxyAPI` 官方 GitHub Release 获取匹配 `darwin/arm64` 的资产。
- Keeper 从 `Willxup/cpa-usage-keeper` 官方 GitHub Release 获取匹配 `darwin/arm64` 的资产。
- 每次下载必须同时取得 Release 中的 `checksums.txt`，并以 `shasum -a 256` 比对。
- 仅接受预设官方仓库、精确资产名和 GitHub 下载地址；缺少校验文件、校验不符、下载失败或二进制 `--help` 自检失败时退出非零，不切换版本。
- 首次安装和 `update.sh` 默认选择两项服务的最新版。安装器可通过环境变量或命令参数接收固定版本，便于可复现安装；具体参数在实现计划中定义。

升级 CPA 后再升级 Keeper。Keeper 运行前会重试本地 CPA，因而不依赖 `launchd` 的硬性启动顺序。

升级 Keeper 前使用 `sqlite3 .backup` 创建一致性 SQLite 备份并输出路径；若 `sqlite3` 不可用，升级拒绝继续而不是复制可能处于 WAL 状态的数据库文件。

## LaunchAgent 与生命周期

安装器向 `$HOME/Library/LaunchAgents/` 写入以下两个用户级 plist：

- `io.cpa-local.cli-proxy-api.plist`
- `io.cpa-local.usage-keeper.plist`

每个 plist 的 `ProgramArguments` 指向运行目录内的稳定启动包装器，包装器再 `exec` 对应的 `current/` 可执行路径。`WorkingDirectory` 指向运行根目录，stdout/stderr 写到 `logs/`，并使用 `RunAtLoad`、`KeepAlive` 和 `ThrottleInterval=10`。

安装通过 `launchctl bootstrap gui/$(id -u)` 注册服务。脚本始终管理当前用户的 launchd domain，不使用 `sudo`。

手动停止和注销登录自启是不同操作：

| 脚本 | 行为 |
| --- | --- |
| `start.sh [cli\|keeper]` | 清除目标停用标记，并 `launchctl kickstart -k`；未安装时提示先运行安装器。无参数表示两项服务。 |
| `stop.sh [cli\|keeper]` | 写入 `state/<service>.disabled`，使 plist 的 `KeepAlive` 条件为假，再发送 `SIGTERM`。服务不会在当前会话反复拉起，但仍保留在登录自启注册中。 |
| `restart.sh [cli\|keeper]` | 清除停用标记后 `kickstart -k`；全部重启按 CPA、Keeper 顺序。 |
| `status.sh` | 显示注册状态、停用状态、当前版本、端口监听与 HTTP 健康检查。 |
| `uninstall.sh` | `bootout` 服务并删除 plist，保留运行目录。 |
| `uninstall.sh --purge` | 在明确二次确认后，注销服务并删除整个运行目录。 |

两个启动包装器固定安装到 `cpa-stack/bin/`。它们在执行服务前检查 `state/<service>.disabled`：存在时以 0 退出；不存在时 `exec` 当前版本二进制。plist 使用 `KeepAlive: { SuccessfulExit: false }`：包装器因停用标记正常退出时不会被重启，而服务意外退出时会被 launchd 重启。`stop.sh` 先写标记再发 `SIGTERM`；即使 launchd 发生一次竞态重启，包装器也会正常退出并停止重试。测试覆盖标记存在、清除和重启场景。

## 健康检查、错误处理与可观察性

- CPA 要求 8317 端口监听且本地 HTTP 连接成功。
- Keeper 要求 18080 端口监听且本地 HTTP 连接成功。若其认证策略使匿名请求非 2xx，非连接错误的 HTTP 响应也视为服务在线。
- `status.sh` 同时输出 launchd job 状态、版本、停用标记、监听状态与检查结论。
- 服务日志写入运行目录的 `logs/`。脚本失败时输出具体原因、下一步操作和相关日志路径。
- 所有变更操作均应幂等：重复安装不覆盖私密状态，重复停止/启动/卸载给出稳定结果并返回合适的退出状态。

默认服务仅绑定本机回环地址：CPA 为 `127.0.0.1:8317`，Keeper 为 `127.0.0.1:18080`。首版不管理 PF，不开放局域网或 EasyTier 访问；此类能力若有需要，作为显式的后续网络功能设计。

## 验证策略

公共 shell 函数应具备可测试接口。测试覆盖：

- 非 macOS 或非 arm64 平台拒绝；
- 缺少命令、配置占位符、错误权限和无效配置；
- 下载失败、缺少 checksum、SHA 不符和二进制自检失败均不切换当前版本；
- 原子软链接切换与显式回滚；
- 服务目标解析、停用标记、启动和停止语义；
- plist 模板渲染与 `plutil -lint`；
- `sh -n` 对全部脚本的语法检查。

macOS CI 使用 dry-run 模式验证安装/更新流程，不真正写入用户 LaunchAgent 或注册服务。真实 `launchctl` 注册和端口检查保留为本机验收步骤。

## README 内容

README 应包含：平台要求、最短安装命令、两种初始化路径、日常运维命令、日志与状态排障、升级和回滚、手工迁移、卸载说明及安全警告。安全警告明确说明不要提交或分享运行目录、配置、认证文件、数据库、日志及下载包。

## 验收标准

在干净的 Apple Silicon macOS 用户环境中，克隆仓库并运行 `sh macos/install.sh --init` 后：

1. 用户能在不安装 zsh、Docker 或 sudo 的情况下完成配置和安装；
2. 两项二进制均来自经 SHA-256 校验的官方 GitHub Release；
3. 两个 LaunchAgent 已注册，登录后可自动启动；
4. `start.sh`、`stop.sh`、`restart.sh`、`status.sh`、`update.sh` 和 `uninstall.sh` 符合本设计的生命周期语义；
5. 失败的下载或升级不会破坏运行中的版本、认证文件、配置或 Keeper 数据库；
6. 仓库中没有任何真实密钥、认证文件、数据库、日志或 release 二进制。
