# CPA 与 usage-keeper Windows x64 裸机部署设计

## 目标与边界

在不依赖 Docker、NSSM、WinSW 或其他第三方守护程序的前提下，让用户克隆仓库后进入 `windows\`，以管理员 PowerShell 执行安装脚本，完成 CPA（CLIProxyAPI）与 cpa-usage-keeper 的下载、配置、开机自启、更新和卸载。

首版范围：

- Windows x64（Windows 10/11 x64、Windows Server 2019 及以上）；
- 真正的系统启动后自启，不依赖用户登录；
- 使用 Windows 内置的 PowerShell、任务计划程序、防火墙、压缩解包和 SHA-256 工具；
- 下载两个项目官方 GitHub Release 中的 Windows AMD64 资产。

不包含 Windows ARM64、Linux、容器部署、反向代理、远程公开访问和自动迁移其他机器的数据。

## 选定架构

使用任务计划程序而不是第三方 Windows Service 包装器。

安装器以管理员权限创建两项 SYSTEM 任务：

- `CPAStack-CLIProxyAPI`
- `CPAStack-UsageKeeper`

任务使用 `AtStartup` 触发器，账户为 `NT AUTHORITY\SYSTEM`，并配置失败后每分钟重试。任务调用运行目录中的稳定 PowerShell 包装器；包装器再运行当前版本二进制。因此任务定义不会随应用升级而改变。

选择该方案的原因：它在无人登录时也可运行、无需引入额外守护程序、可由 PowerShell 原生创建和删除。代价是安装、更新、启停和卸载均需提升权限。

## 仓库布局

```text
CPAOps/
├── config/
│   ├── cpa.config.yaml.example
│   └── keeper.env.example
├── macos/                         # 现有 macOS 实现，不在本方案改动范围内
├── windows/
│   ├── install.cmd
│   ├── install.ps1
│   ├── start.ps1
│   ├── stop.ps1
│   ├── restart.ps1
│   ├── status.ps1
│   ├── update.ps1
│   ├── uninstall.ps1
│   ├── proxy.ps1
│   ├── lib/
│   │   ├── Common.ps1
│   │   ├── Config.ps1
│   │   ├── GitHubRelease.ps1
│   │   ├── ScheduledTask.ps1
│   │   └── Firewall.ps1
│   └── tasks/
│       ├── Run-CLIProxyAPI.ps1
│       └── Run-UsageKeeper.ps1
└── tests/windows/
```

脚本全部以 PowerShell 5.1 兼容语法编写，并可在 PowerShell 7 运行。Windows 与 macOS 使用相同的运维动词、服务目标和默认行为；仅安装入口不同：`install.cmd` 负责首次执行策略引导和 UAC 提升，之后的运维脚本均直接以 `.ps1` 运行。

## 私有运行目录与 ACL

运行根目录固定为：`C:\ProgramData\CPAStack`。

```text
C:\ProgramData\CPAStack\
├── config\
│   ├── config.yaml
│   ├── keeper.env
│   ├── proxy.psd1
│   └── github-token
├── auths\
├── keeper\
│   ├── app.db
│   └── backups\
├── releases\
│   ├── cli-proxy-api\<version>\
│   └── cpa-usage-keeper\<version>\
├── current\
│   ├── cli-proxy-api\
│   └── cpa-usage-keeper\
├── logs\
├── state\
└── tasks\
```

`current\` 下的两个目录 junction 分别指向已激活版本。`SYSTEM` 与 `BUILTIN\Administrators` 有完全控制权；普通用户不具有 `config\`、`auths\`、`keeper\`、`logs\`、`state\` 的读取或写入权限。安装器使用 `icacls` 显式设置继承与 ACL，而不依赖 `ProgramData` 默认权限。

仓库不保存真实配置、认证文件、数据库、token、代理、日志、压缩包或二进制。

## 安装流程

入口：

```powershell
cd .\windows
.\install.cmd
```

安装器按顺序执行：

1. `install.cmd` 用临时 `Bypass` 调用 `Set-ExecutionPolicy -Scope CurrentUser RemoteSigned -Force`，再请求 UAC 提升启动 `install.ps1`；执行策略只修改当前用户，不修改 LocalMachine。若企业组策略强制限制执行策略，安装器明确报出该限制。
2. 检查管理员权限、Windows x64、PowerShell、`schtasks`、`Expand-Archive`、`Get-FileHash`、`Test-NetConnection` 和任务计划程序服务可用。
3. 创建运行目录和受限 ACL；不覆盖已有 `config.yaml`、`keeper.env`、`auths\` 或 `keeper\app.db`。
4. 首次缺少配置时，以安全输入框读取 CPA 管理密钥和 Keeper 登录密码，使用模板生成私有配置。CPA 使用 `127.0.0.1:8317`；Keeper 的 `CPA_BASE_URL`、`CPA_PUBLIC_URL` 与 Redis 地址均指向该本机地址。
5. 加载当前进程代理或已保存代理；两者都不存在时允许粘贴 `export https_proxy=... http_proxy=... all_proxy=...`、`set HTTPS_PROXY=...` 或 PowerShell 环境变量格式。解析器只接受三项代理变量与 `http://`、`https://`、`socks5://` URL，绝不执行粘贴文本。
6. 下载并校验两个 Release，创建/刷新包装器和 SYSTEM 任务，启动任务，并检查端口和 HTTP 连通性。

重复运行安装器不会覆盖私密状态；它会复用配置、重新检查最新版、刷新任务定义并确保服务已启动。

## GitHub 下载、代理与 token

固定官方来源：

| 服务 | 仓库 | Windows x64 资产模式 |
| --- | --- | --- |
| CPA | `router-for-me/CLIProxyAPI` | `CLIProxyAPI_<version>_windows_amd64.zip` |
| Keeper | `Willxup/cpa-usage-keeper` | `cpa-usage-keeper_v<version>_windows_amd64.zip` |

下载逻辑：

1. 首先直连 GitHub；
2. 仅收到 HTTP 401 或 403 时读取 `config\github-token` 重试；
3. token 不存在或重试仍为 401/403 时，以安全输入读取新 token、原子覆盖私有 token 文件并重试；
4. 获取匹配资产和 `checksums.txt`，使用 `Get-FileHash -Algorithm SHA256` 校验；
5. 校验、解压、二进制自检全部成功后才允许激活版本。

代理存为 `config\proxy.psd1`，由安装/更新脚本使用；SYSTEM 任务包装器同样加载该文件并写入子进程环境，避免依赖某个用户的环境变量。token 与代理文件继承私有 ACL。

## 服务生命周期

包装器在 `tasks\Run-CLIProxyAPI.ps1` 与 `tasks\Run-UsageKeeper.ps1`：

1. 检查 `state\<service>.disabled`，存在则以成功代码退出；
2. 读取保存代理并设置子进程环境；
3. 从 `current\` 目录启动相应二进制，传入稳定配置路径；
4. 把标准输出与错误输出追加到 `logs\`。

PowerShell 运维接口与 macOS 一致：

```powershell
.\start.ps1 [cli|keeper]
.\stop.ps1 [cli|keeper]
.\restart.ps1 [cli|keeper]
.\status.ps1
.\update.ps1 [cli|keeper]
.\proxy.ps1 set|clear|show
.\uninstall.ps1 [--purge]
```

无服务参数时，`start`、`stop`、`restart`、`update` 操作 CPA 和 Keeper 全部服务。服务名仅接受 `cli`、`keeper` 或默认的全部服务，和 macOS 的命令语义一致。`--purge` 需要输入 `DELETE`；PowerShell 参数实现同样接受 `-Purge` 作为等价别名，便于 Windows 管理习惯。

停止操作先写停用标记，再 `Stop-ScheduledTask`，从而避免任务重试。启动/重启操作删除标记并 `Start-ScheduledTask`。卸载才删除任务；`-Purge` 要求输入精确的 `DELETE`，才删除整个运行目录。

## 更新与回滚

更新脚本以管理员权限运行：

1. 停止 Keeper 任务后，备份 `app.db` 及可能存在的 `app.db-wal`、`app.db-shm` 文件；Windows 首版不依赖未随系统提供的 `sqlite3.exe`；
2. 下载、校验、解压和运行 `--help` 自检；
3. 停止目标任务；
4. 创建 `current.next` junction，替换当前 junction，并保留 `current.previous`；
5. 启动任务、等待端口监听并进行本地 HTTP 检查；
6. 检查失败时停止任务、恢复 `current.previous`、启动旧版本，并返回非零。

更新输出每项服务的检查版本、激活版本和最终状态。首版提供 `-Rollback -Service <service> -Version <version>`，只允许回滚到 `releases\` 中已验证的版本。

## 网络与防火墙

CPA 配置固定绑定 `127.0.0.1:8317`。Keeper 可能监听所有接口，安装器创建命名为 `CPAStack-Block-Remote-Keeper` 的 Windows 入站阻止规则，限制 TCP 18080 的远程访问；本机访问不受该规则影响。卸载时删除该规则。

安装器不会创建任何入站允许规则、端口转发或公网暴露。若后续需要局域网访问，应另行设计显式的防火墙范围与认证策略。

## 状态、日志与错误处理

`Get-CPAStackStatus.ps1` 显示：任务计划状态、停用标记、激活版本、8317/18080 监听情况和本地 HTTP 检查结果。失败的操作提供日志位置：

```text
C:\ProgramData\CPAStack\logs\cli-proxy-api.out.log
C:\ProgramData\CPAStack\logs\cli-proxy-api.err.log
C:\ProgramData\CPAStack\logs\cpa-usage-keeper.out.log
C:\ProgramData\CPAStack\logs\cpa-usage-keeper.err.log
```

下载、checksum、解压、自检、任务注册和健康检查中任一步失败均返回非零；升级失败不得破坏已激活版本、认证文件、配置或数据库。

## 测试与验收

测试使用仓库自带 PowerShell 测试 harness，不强制安装 Pester。覆盖：

- 非管理员、非 Windows、非 x64 环境拒绝；
- 运行目录 ACL、配置占位符、代理解析和 token 文件权限；
- 401/403 的匿名→token 重试、checksum 不符、错误资产名和下载失败；
- current junction 切换、失败回滚与调用方变量隔离；
- 任务 XML/注册参数、停用标记、启停/卸载语义；
- 防火墙规则创建与删除；
- 安装/更新的 dry-run，保证不写真实任务或防火墙。

Windows x64 验收标准：在干净的管理员 PowerShell 中执行 `install.ps1` 后，两个任务在重启且无人登录时启动，CPA 可在 `127.0.0.1:8317` 访问，Keeper 可在本机 18080 访问；认证、数据库和敏感配置不出现在仓库；失败更新保持旧版本可用。
