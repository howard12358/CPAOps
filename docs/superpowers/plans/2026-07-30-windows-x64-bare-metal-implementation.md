# Windows x64 Bare-Metal Deployment Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a Windows x64 native deployment path for CPA and Keeper with SYSTEM startup tasks and macOS-aligned operator commands.

**Architecture:** PowerShell libraries own validation, secure runtime state, GitHub release activation, scheduled tasks and firewall configuration. `install.cmd` bootstraps CurrentUser `RemoteSigned` and UAC elevation, while `install.ps1` through `uninstall.ps1` use the same command verbs and targets as macOS.

**Tech Stack:** Windows PowerShell 5.1, Task Scheduler, Windows Firewall, `Get-FileHash`, `Expand-Archive`, PowerShell test harness.

## Global Constraints

- Support only Windows x64; require administrator elevation for state-changing commands.
- Install to `C:\ProgramData\CPAStack`; restrict sensitive data to SYSTEM and Administrators.
- Use no Docker, NSSM, WinSW, Homebrew, external package manager, or third-party PowerShell module.
- Download only official `windows_amd64.zip` assets and require `checksums.txt` SHA-256 verification.
- Register `CPAStack-CLIProxyAPI` and `CPAStack-UsageKeeper` as SYSTEM `AtStartup` scheduled tasks.
- Preserve configuration, auth files and database on repeated install; only `--purge` may delete runtime state.

---

### Task 1: Windows skeleton, bootstrap, and native test harness

**Files:**
- Create: `windows/install.cmd`, `windows/install.ps1`, `windows/lib/Common.ps1`
- Create: `tests/windows/TestHelpers.ps1`, `tests/windows/Run-Tests.ps1`, `tests/windows/Common.Tests.ps1`

**Interfaces:**
- `Assert-WindowsX64`, `Assert-Administrator`, `Get-CPAStackRoot`, `Initialize-CPAStackLayout`.
- `install.cmd` sets CurrentUser `RemoteSigned`, checks policy result, requests UAC, then invokes `install.ps1`.

- [ ] Write a failing test that verifies a non-x64 override is rejected and layout includes `config`, `auths`, `keeper`, `releases`, `current`, `logs`, `state`, and `tasks`.
- [ ] Run `powershell -NoProfile -ExecutionPolicy Bypass -File .\tests\windows\Run-Tests.ps1` on Windows; expect missing-command failures.
- [ ] Implement the minimal functions and a no-dependency assertion harness (`Assert-Equal`, `Assert-Throws`, `Assert-Path`).
- [ ] Run the harness and commit `chore: scaffold Windows deployment support`.

### Task 2: Configuration, ACL, proxy, and GitHub token handling

**Files:**
- Create: `windows/lib/Config.ps1`, `windows/lib/Network.ps1`
- Create: `tests/windows/Config.Tests.ps1`, `tests/windows/Network.Tests.ps1`

**Interfaces:**
- `Initialize-PrivateAcl`, `Initialize-Config`, `Import-CPAStackProxy`, `Set-CPAStackProxy`, `Invoke-GitHubRequest`.
- `Invoke-GitHubRequest` tries anonymous access, retries saved token only for 401/403, and prompts/replaces token only for interactive 401/403.

- [ ] Write failing tests for placeholder rejection, accepted `export`/`set` proxy lines, `proxy.psd1` restricted path, and an injected HTTP client returning 403 then 200.
- [ ] Run the Windows harness; expect functions missing.
- [ ] Implement safe parsing without `Invoke-Expression`; save `proxy.psd1` and `github-token` beneath `config`; apply SYSTEM/Administrators-only ACL.
- [ ] Re-run tests and commit `feat: add Windows private configuration and network helpers`.

### Task 3: Verified release activation and rollback primitives

**Files:**
- Create: `windows/lib/GitHubRelease.ps1`
- Create: `tests/windows/Releases.Tests.ps1`

**Interfaces:**
- `Get-LatestRelease`, `Install-VerifiedRelease`, `Set-CurrentRelease`, `Restore-PreviousRelease`.
- Services: `cli` maps to `CLIProxyAPI_<version>_windows_amd64.zip` / `cli-proxy-api.exe`; `keeper` maps to `cpa-usage-keeper_v<version>_windows_amd64.zip` / `cpa-usage-keeper.exe`.

- [ ] Write failing tests using local zip fixtures: checksum mismatch preserves `current`; a valid binary fixture creates a `current` junction; restore returns to the old version.
- [ ] Run the tests; expect missing functions.
- [ ] Implement SHA-256 comparison, temporary extraction, executable self-check, `current.next`/`current.previous` junction swap, and release-only rollback.
- [ ] Re-run tests and commit `feat: add verified Windows release activation`.

### Task 4: SYSTEM task wrappers, firewall, and lifecycle commands

**Files:**
- Create: `windows/lib/ScheduledTask.ps1`, `windows/lib/Firewall.ps1`, `windows/tasks/Run-CLIProxyAPI.ps1`, `windows/tasks/Run-UsageKeeper.ps1`
- Create: `windows/start.ps1`, `windows/stop.ps1`, `windows/restart.ps1`, `windows/status.ps1`, `windows/proxy.ps1`, `windows/uninstall.ps1`
- Create: `tests/windows/Lifecycle.Tests.ps1`

**Interfaces:**
- `Register-CPAStackTasks`, `Start-CPAStackService`, `Stop-CPAStackService`, `Get-CPAStackServiceStatus`, `Set-CPAStackFirewall`.

- [ ] Write failing tests with injected Task Scheduler/Firewall adapters: target parsing, disabled marker behavior, SYSTEM AtStartup task settings, and named Keeper inbound block rule.
- [ ] Run the harness; expect missing functions.
- [ ] Implement task registration, wrapper redirection, disabled markers, task start/stop, status, proxy set/clear/show, uninstall and guarded `--purge`/`-Purge`.
- [ ] Re-run tests and commit `feat: add Windows service lifecycle commands`.

### Task 5: Install/update orchestration, documentation, and Windows acceptance

**Files:**
- Modify: `windows/install.ps1`
- Create: `windows/update.ps1`, `tests/windows/InstallUpdate.Tests.ps1`, `windows/README.md`
- Modify: root `README.md`

**Interfaces:**
- `install.ps1` has no operational parameter and initializes only absent configuration.
- `update.ps1 [cli|keeper]` supports `-Rollback -Service <service> -Version <version>` and reports checked/active versions.

- [ ] Write failing dry-run tests proving install does not overwrite config/database and failed update restores the prior current release.
- [ ] Run harness; expect orchestration commands missing.
- [ ] Implement installation order, Keeper SQLite `.backup`, task stop/swap/start/health check rollback, explicit progress output, and README command examples.
- [ ] Run every Windows test on a Windows x64 administrator host; additionally reboot without logging in and verify both SYSTEM tasks and ports.
- [ ] Commit `feat: add Windows x64 native deployment`.
