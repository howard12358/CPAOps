# Windows x64 裸机部署实施计划

> **供自动化执行者使用：** 必须使用 `superpowers:subagent-driven-development`（推荐）或 `superpowers:executing-plans`，按任务逐项执行。本计划用复选框跟踪状态。

**目标：** 为 CPA 和 Keeper 增加 Windows x64 原生部署路径，使用 SYSTEM 启动任务，并与 macOS 保持一致的运维命令。

**架构：** PowerShell 库分别负责平台与权限校验、私有运行目录、配置、GitHub Release、计划任务与防火墙；`install.cmd` 负责首次执行策略引导与 UAC 提升，`install.ps1` 至 `uninstall.ps1` 使用与 macOS 相同的命令动词和服务目标。

**技术栈：** Windows PowerShell 5.1、任务计划程序、Windows 防火墙、`Get-FileHash`、`Expand-Archive`、仓库内 PowerShell 测试 harness。

## 全局约束

- 仅支持 Windows x64；所有修改状态的操作必须在管理员 PowerShell 中执行。
- 安装到 `C:\ProgramData\CPAStack`；敏感数据仅允许 SYSTEM 与 Administrators 访问。
- 不使用 Docker、NSSM、WinSW、第三方包管理器或第三方 PowerShell 模块。
- 仅下载官方 `windows_amd64.zip` 资产，且必须由 `checksums.txt` 验证 SHA-256。
- 注册 `CPAStack-CLIProxyAPI`、`CPAStack-UsageKeeper` 两项 SYSTEM `AtStartup` 计划任务。
- 重复安装不得覆盖配置、认证文件和数据库；只有 `--purge` 可以删除运行态数据。

## 当前状态

- [x] 已创建 Windows 脚本骨架：`install.cmd`、安装/启停/状态/更新/代理/卸载入口、基础库和任务包装器。
- [ ] 尚未在 Windows x64 管理员环境执行 PowerShell 语法检查、测试或真实任务计划验收。
- [ ] 以下任务均未达到“测试通过且可验收”的完成标准。

---

### 任务 1：Windows 骨架、安装引导与原生测试 harness

**文件：**

- 创建：`windows/install.cmd`、`windows/install.ps1`、`windows/lib/Common.ps1`
- 创建：`tests/windows/TestHelpers.ps1`、`tests/windows/Run-Tests.ps1`、`tests/windows/Common.Tests.ps1`

**接口：**

- `Assert-WindowsX64`、`Assert-Administrator`、`Get-CPAStackRoot`、`Initialize-CPAStackLayout`。
- `install.cmd` 设置当前用户的 `RemoteSigned`，检查设置结果、请求 UAC，随后调用 `install.ps1`。

- [x] 编写失败测试：非 x64 覆盖值必须被拒绝；运行目录必须包含 `config`、`auths`、`keeper`、`releases`、`current`、`logs`、`state`、`tasks`。
- [ ] 在 Windows 执行 `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\windows\Run-Tests.ps1`，确认因缺少命令而失败。
- [x] 实现最小函数与无依赖断言 harness：`Assert-Equal`、`Assert-Throws`、`Assert-Path`。
- [ ] 运行 harness 并确认通过。
- [x] 提交：`feat(windows): 增加基础校验与测试框架`。

### 任务 2：配置、ACL、代理和 GitHub token

**文件：**

- 创建：`windows/lib/Config.ps1`、`windows/lib/Network.ps1`
- 创建：`tests/windows/Config.Tests.ps1`、`tests/windows/Network.Tests.ps1`

**接口：**

- `Initialize-PrivateAcl`、`Initialize-Config`、`Import-CPAStackProxy`、`Set-CPAStackProxy`、`Invoke-GitHubRequest`。
- `Invoke-GitHubRequest` 先匿名访问；仅在 401/403 时使用已保存 token 重试；交互式 401/403 时才提示并替换 token。

- [x] 编写失败测试：配置占位符拒绝、`export`/`set` 代理格式解析、`proxy.psd1` 私有路径、注入 HTTP 客户端的 403→200 token 重试。
- [ ] 运行 Windows harness，确认函数缺失导致失败。
- [x] 实现安全解析，禁止 `Invoke-Expression`；将 `proxy.psd1`、`github-token` 存于 `config`，并设置 SYSTEM/Administrators 专属 ACL。
- [ ] 重跑测试并确认通过。
- [x] 提交：`feat(windows): 增加配置与网络辅助函数`。

### 任务 3：校验下载、版本激活与回滚原语

**文件：**

- 创建：`windows/lib/GitHubRelease.ps1`
- 创建：`tests/windows/Releases.Tests.ps1`

**接口：**

- `Get-LatestRelease`、`Install-VerifiedRelease`、`Set-CurrentRelease`、`Restore-PreviousRelease`。
- `cli` 对应 `CLIProxyAPI_<version>_windows_amd64.zip` / `cli-proxy-api.exe`；`keeper` 对应 `cpa-usage-keeper_v<version>_windows_amd64.zip` / `cpa-usage-keeper.exe`。

- [x] 使用本地 zip fixture 编写失败测试：checksum 不符必须保持 `current`；有效二进制创建 `current` junction；恢复必须回到旧版本。
- [ ] 运行测试，确认接口缺失。
- [x] 实现 SHA-256 比对、临时解压、二进制自检、`current.next`/`current.previous` junction 切换和仅限已验证版本的回滚。
- [ ] 重跑测试并确认通过。
- [ ] 提交：`feat: 增加 Windows 已校验版本激活`。

### 任务 4：SYSTEM 任务包装器、防火墙与生命周期命令

**文件：**

- 创建：`windows/lib/ScheduledTask.ps1`、`windows/lib/Firewall.ps1`、`windows/tasks/Run-CLIProxyAPI.ps1`、`windows/tasks/Run-UsageKeeper.ps1`
- 创建：`windows/start.ps1`、`windows/stop.ps1`、`windows/restart.ps1`、`windows/status.ps1`、`windows/proxy.ps1`、`windows/uninstall.ps1`
- 创建：`tests/windows/Lifecycle.Tests.ps1`

**接口：**

- `Register-CPAStackTasks`、`Start-CPAStackService`、`Stop-CPAStackService`、`Get-CPAStackServiceStatus`、`Set-CPAStackFirewall`。

- [x] 使用可注入的任务计划/防火墙适配器编写失败测试：服务目标解析、停用标记、SYSTEM `AtStartup` 设置、Keeper 入站阻止规则。
- [ ] 运行 harness，确认接口缺失。
- [x] 实现任务注册、包装器日志重定向、停用标记、启停、状态、代理设置/清除/查看、卸载与受保护的 `--purge`/`-Purge`。
- [ ] 重跑测试并确认通过。
- [ ] 提交：`feat: 增加 Windows 服务生命周期命令`。

### 任务 5：安装/更新编排、文档与 Windows 验收

**文件：**

- 修改：`windows/install.ps1`
- 创建：`windows/update.ps1`、`tests/windows/InstallUpdate.Tests.ps1`、`windows/README.md`
- 修改：根目录 `README.md`

**接口：**

- `install.ps1` 不接受运维参数，只初始化缺失配置。
- `update.ps1 [cli|keeper]` 支持 `-Rollback -Service <service> -Version <version>`，并输出检查版本和激活版本。

- [ ] 编写失败 dry-run 测试：安装不得覆盖配置/数据库；更新失败必须恢复此前 `current` 版本。
- [ ] 运行 harness，确认编排接口缺失。
- [ ] 实现安装顺序、Keeper SQLite `.backup`、停止任务/切换/启动/健康检查回滚、明确进度输出及 README 示例。
- [ ] 在 Windows x64 管理员主机执行所有 Windows 测试；重启且不登录，验证两个 SYSTEM 任务和端口。
- [ ] 提交：`feat: 完成 Windows x64 原生部署`。
