# CPA 跨平台 Rust CLI 设计

## 目标

以单个 Rust 二进制 `cpactl` 替代仓库中 macOS Shell 与 Windows PowerShell 运维脚本的实现。首期支持 macOS Apple Silicon 与 Windows x64，并保持现有 CPA（CLIProxyAPI）和 Keeper（cpa-usage-keeper）的部署行为与数据布局。

目标是统一命令、升级安全性、错误输出和自动化接口；不是将两个操作系统强行使用同一种服务管理机制。

## 范围

首期实现以下命令：

```text
cpactl install
cpactl start [cli|keeper]
cpactl stop [cli|keeper]
cpactl restart [cli|keeper]
cpactl status [--json]
cpactl logs <cli|keeper> [-f] [-n 200]
cpactl update [cli|keeper]
cpactl rollback <cli|keeper> --version <version>
cpactl proxy set|show|clear
cpactl path
cpactl uninstall [--purge]
```

命令接受 `cli` / `cli-proxy-api` 与 `keeper` / `cpa-usage-keeper` 两组等价名称。无服务参数时，生命周期与更新命令作用于全部服务。

首期不包括 Linux、自动配置编辑器、自动迁移旧安装目录、远程控制、GUI 和自更新 `cpactl` 本身。

## 运行与权限模型

| 平台 | 支持架构 | 默认根目录 | 服务托管方式 | 权限要求 |
| --- | --- | --- | --- | --- |
| macOS | Apple Silicon (`aarch64`) | `~/Library/Application Support/cpa-stack` | 当前用户 LaunchAgent | 安装不需要 sudo |
| Windows | x64 | `C:\ProgramData\CPAStack` | SYSTEM 的开机计划任务 | 安装及管理需要管理员 |

两种模型保持现状：macOS 用户级服务符合系统约定且不需要提权；Windows 的 SYSTEM 开机任务能在无人登录时运行，并使服务身份、数据目录与 ACL 稳定。首期不改用 Windows Service。

`CPA_STACK_ROOT` 与全局 `--root <path>` 均可覆盖默认目录，后者优先，用于隔离测试、迁移和高级部署。根目录包含：

```text
config/       CPA 配置、Keeper 环境变量、代理和 GitHub Token
auths/        CPA 认证文件
keeper/       Keeper 数据库和更新备份
releases/     按服务与版本组织的已验证二进制
current/      当前生效版本的链接（macOS 符号链接；Windows 目录联接）
downloads/    临时下载与 Release 元数据
logs/         两个服务的标准输出与错误日志
state/        手动停止标记及事务恢复信息
bin/          macOS LaunchAgent 使用的启动包装器
tasks/        Windows 计划任务的启动脚本或配置
```

## 架构

单 crate 起步，按模块隔离，不在首期提前拆成 workspace。所有命令调用共享核心接口；操作系统细节只存在于平台适配层。

```text
src/
  main.rs             进程入口、退出码映射
  cli.rs              Clap 命令与参数定义
  app.rs              命令编排、输出模型
  domain/
    service.rs        服务目录、别名、端口、二进制与 GitHub 仓库
    release.rs        版本、资产、校验和、激活事务
    runtime.rs        运行目录与状态模型
    error.rs          稳定领域错误
  storage/
    config.rs         配置、代理、Token 与密钥脱敏
    filesystem.rs     原子文件/目录操作
  github.rs           Release API、下载与认证回退
  platform/
    mod.rs            生命周期、权限、网络、链接的抽象接口
    macos.rs          LaunchAgent、POSIX 权限、lsof
    windows.rs        计划任务、ACL、防火墙、Get-NetTCPConnection
  output.rs           人类可读与 JSON 输出
```

`ServiceCatalog` 是唯一的服务定义来源。每项服务包含 GitHub 仓库、按平台的资产匹配规则、二进制名、端口、日志名、启动参数与服务管理标签。任何安装、下载、状态或日志代码不得各自维护服务字符串。

`Platform` 接口负责：检查支持的平台与权限、安装/移除服务定义、启动/停止/重启、读取托管状态、建立原子 current 链接、锁定文件权限、检查端口和配置防火墙。核心层不依赖 LaunchAgent、计划任务、ACL 或防火墙 API。

## 安装、配置与网络

`install` 创建运行布局并询问 CPA 管理密钥和 Keeper 登录密码；如果配置文件已存在，则不覆盖已有内容。非交互模式通过显式参数或环境变量提供这两个值，缺失时失败，不在终端回显密钥。

配置模板沿用现有 `config/cpa.config.yaml.example` 与 `config/keeper.env.example`。安装前验证：无 `__REQUIRED__` 占位符、CPA 和 Keeper 端口合法、私密文件权限正确。

代理保存为平台无关的结构化 TOML 文件（键为 `http_proxy`、`https_proxy`、`all_proxy`）。读取时只将其注入当前 `cpactl` 下载进程的 HTTP 客户端，不修改全局系统代理或永久环境变量。

GitHub 请求先匿名；仅收到 401/403 时使用已保存 Token，再在交互终端中请求 Token。Token 必须保存为私密文件，且日志、JSON 输出、错误文本一律只显示其是否已配置。

## Release、更新与回滚事务

每次安装或更新对每项服务按以下顺序执行：

1. 查询 GitHub 最新 Release，并按当前平台选择唯一目标资产和 `checksums.txt`。
2. 下载到 `downloads/` 的唯一临时文件；下载完成后再改名，避免半成品被误用。
3. 从校验文件精确匹配资产名，计算 SHA-256；不匹配即删除临时文件并终止。
4. 解压至版本目录的同级临时目录，找到预期二进制，并验证其可启动（例如 `--help`）。
5. 将临时目录原子改名为 `releases/<service>/<version>`；已存在且经验证的版本可复用。
6. 保存当前版本与运行状态；Keeper 在停止前备份 SQLite 数据库及 WAL/SHM 文件。
7. 切换 `current/<service>` 到目标版本：macOS 使用临时符号链接再重命名；Windows 使用临时目录联接并替换。
8. 启动或重启原本运行的服务，等待端口开始监听；超时或启动失败则切回原版本并恢复原运行状态。

更新所有服务时逐项执行，各服务结果分别报告。失败服务会自动回退；已经成功更新的其他服务不自动降级。`rollback` 只能切换本机 `releases/` 中已验证的版本，执行相同的服务重启和健康检查流程。

## 状态、日志与卸载

`status` 为每项服务显示：逻辑状态（运行、已停止、未安装、已禁用、异常）、托管器状态、当前版本、端口监听和运行目录。`status --json` 输出稳定结构，供脚本消费，永不携带密钥、代理 URL 或 Token。

`logs` 默认显示两个日志文件的最后 200 行；`-f` 持续跟随。Windows 与 macOS 都使用 `logs/<service>.out.log` 和 `logs/<service>.err.log` 的统一文件名。

`stop` 写入禁用标记后停止托管器，避免其自动拉起；`start` 与 `restart` 清除该标记。`uninstall` 移除 LaunchAgent 或计划任务；Windows 同时移除 Keeper 防火墙规则。默认保留数据，`--purge` 必须在交互式输入 `DELETE` 后才删除经过根目录边界校验的运行目录。

## 输出、错误与退出码

默认输出为中文人类可读文本；`--json` 返回稳定的成功或失败对象，至少包括 `ok`、`code`、`message` 和与命令相关的非敏感数据。

| 退出码 | 含义 |
| --- | --- |
| 0 | 成功 |
| 2 | 参数或命令用法错误 |
| 3 | 权限不足 |
| 4 | 未安装或当前状态不允许该操作 |
| 5 | 网络、代理或 GitHub 访问失败 |
| 6 | Release 资产、校验和或解压验证失败 |
| 7 | 服务托管、端口健康检查或回滚失败 |
| 1 | 其他内部错误 |

错误应指出可执行的修复动作，例如管理员要求、端口冲突、缺少版本或 GitHub 鉴权失败；不得把底层命令的敏感参数原样泄漏。

## 依赖建议

- `clap`：命令与帮助文本。
- `tokio`：异步下载、进程调用和并发日志处理。
- `reqwest` + `serde` / `serde_json`：GitHub API 与 JSON。
- `sha2`：SHA-256 校验。
- `tar`、`flate2`、`zip`：Release 解包。
- `tempfile`、`fs2`：临时文件和跨进程安装锁。
- `toml`、`serde_yaml`：代理与配置验证。
- `thiserror`：领域错误。
- `tracing` + `tracing-subscriber`：脱敏诊断日志。

平台调用首期可使用 `std::process::Command` 封装原生命令；Windows ACL 与任务注册优先使用 PowerShell/CIM 命令，待接口稳定后再评估 Windows API crate。此选择能先复用成熟的系统接口，并使平台实现可替换。

## 测试策略与验收

1. 核心单元测试：服务别名、资产匹配、版本解析、路径、配置校验、脱敏、退出码与事务状态转移。
2. 文件系统集成测试：临时根目录中验证校验失败不激活、链接切换原子性、失败恢复和 Keeper 备份。
3. GitHub 客户端测试：本地 HTTP 假服务验证匿名请求、401/403 Token 回退、代理配置和下载错误。
4. 平台适配器契约测试：以命令执行器替身断言 macOS 的 LaunchAgent 调用及 Windows 的计划任务、ACL、防火墙调用。
5. 真实系统冒烟测试：macOS Apple Silicon 与 Windows x64 分别覆盖安装、启停、更新、回滚、日志、卸载和 `--purge` 确认。

验收标准是：任一平台的所有公开命令拥有相同名称、参数与退出码；二进制和配置不依赖仓库路径；更新校验失败或服务起不来时当前稳定版本仍可用；任何默认或 JSON 输出均不泄露密钥。
