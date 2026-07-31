# Windows x64 部署

在管理员 PowerShell 中进入本目录，首次安装执行：

```powershell
.\install.cmd
```

之后命令与 macOS 保持一致：

```powershell
.\start.ps1 [cli|keeper]
.\stop.ps1 [cli|keeper]
.\restart.ps1 [cli|keeper]
.\status.ps1
.\logs.ps1 cli
.\logs.ps1 keeper
.\stack-path.ps1
.\update.ps1 [cli|keeper]
.\update.ps1 -Rollback -RollbackService cli -Version 1.2.3
.\proxy.ps1 set|clear|show
.\uninstall.ps1 [-Purge]
```

`stack-path.ps1` 只输出运行目录绝对路径，便于自行执行 `cd`。

运行数据位于 `C:\ProgramData\CPAStack`。安装会创建 SYSTEM 开机任务，且 Keeper 的 18080 入站访问会被 Windows 防火墙阻止。Windows x64 实机验证命令：

```powershell
powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\windows\Run-Tests.ps1
```
