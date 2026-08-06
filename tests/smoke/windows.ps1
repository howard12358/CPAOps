$ErrorActionPreference = 'Stop'

if ([Environment]::OSVersion.Platform -ne [PlatformID]::Win32NT) {
    throw '此脚本只能在 Windows x64 上运行。'
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = [Security.Principal.WindowsPrincipal]::new($identity)
if (-not $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)) {
    throw '请在提升权限的管理员 PowerShell 中运行冒烟测试。'
}

$repoRoot = (Resolve-Path (Join-Path $PSScriptRoot '../..')).Path
$temporaryRoot = Join-Path ([IO.Path]::GetTempPath()) ("cpactl-smoke-" + [guid]::NewGuid())
$runtimeRoot = Join-Path $temporaryRoot 'cpa-stack'
$binary = Join-Path $repoRoot 'target/debug/cpactl.exe'
$previousPlatformIsolation = $env:CPACTL_SMOKE_NO_PLATFORM_COMMANDS
$env:CPACTL_SMOKE_NO_PLATFORM_COMMANDS = '1'

try {
    Push-Location $repoRoot
    try {
        $hostTriple = (rustc -vV | Where-Object { $_ -like 'host:*' }) -replace '^host:\s*', ''
        if ($hostTriple -ne 'x86_64-pc-windows-msvc') { throw "Rust 目标三元组不受支持：$hostTriple" }
        cargo build --offline --quiet
        if ($LASTEXITCODE -ne 0) { throw '无法构建 cpactl。' }
    }
    finally {
        Pop-Location
    }

    $help = & $binary '--help'
    if ($LASTEXITCODE -ne 0 -or $help -notmatch 'rollback') { throw '帮助输出缺少 rollback。' }
    $pathOutput = & $binary '--root' $runtimeRoot 'path'
    if ($LASTEXITCODE -ne 0 -or $pathOutput -ne $runtimeRoot) { throw 'path 未输出指定的运行目录。' }

    $previousProxy = $env:HTTPS_PROXY
    try {
        $env:HTTPS_PROXY = 'http://smoke-user:smoke-secret@127.0.0.1:0'
        & $binary '--root' $runtimeRoot 'proxy' 'set' | Out-Null
        if ($LASTEXITCODE -ne 0) { throw '无法保存临时代理配置。' }
    }
    finally {
        if ($null -eq $previousProxy) { Remove-Item Env:HTTPS_PROXY -ErrorAction SilentlyContinue }
        else { $env:HTTPS_PROXY = $previousProxy }
    }
    $proxyJson = & $binary '--root' $runtimeRoot '--json' 'proxy' 'show'
    if ($LASTEXITCODE -ne 0 -or $proxyJson -notmatch '"configured":true') { throw '代理状态 JSON 无效。' }
    if ($proxyJson -match 'smoke-secret') { throw '代理密钥泄露到 JSON 输出。' }

    $env:CPA_MANAGEMENT_KEY = 'smoke-management-key'
    $env:KEEPER_LOGIN_PASSWORD = 'smoke-keeper-password'
    try {
        & $binary '--root' $runtimeRoot 'install' *> $null
        if ($LASTEXITCODE -ne 5) { throw "安装未在受控代理处停止，退出码：$LASTEXITCODE" }
    }
    finally {
        Remove-Item Env:CPA_MANAGEMENT_KEY -ErrorAction SilentlyContinue
        Remove-Item Env:KEEPER_LOGIN_PASSWORD -ErrorAction SilentlyContinue
    }
    foreach ($configPath in @(
        (Join-Path $runtimeRoot 'config/config.yaml'),
        (Join-Path $runtimeRoot 'config/keeper.env')
    )) {
        if (-not (Test-Path $configPath -PathType Leaf)) { throw "配置未初始化：$configPath" }
        if (Select-String -Path $configPath -Pattern '__REQUIRED__' -SimpleMatch -Quiet) { throw "配置仍有占位符：$configPath" }
    }
    if (Test-Path (Join-Path $runtimeRoot 'downloads/cli-proxy-api')) { throw '安装意外下载了 CLIProxyAPI。' }

    $statusJson = & $binary '--root' $runtimeRoot '--json' 'status'
    if ($LASTEXITCODE -ne 0 -or $statusJson -notmatch '"ok":true' -or $statusJson -notmatch '"services"') {
        throw 'status JSON 无效。'
    }

    New-Item -ItemType Directory -Force -Path (Join-Path $runtimeRoot 'logs') | Out-Null
    Set-Content -NoNewline -Path (Join-Path $runtimeRoot 'logs/cli-proxy-api.out.log') -Value "old-line`nsmoke-out`n"
    Set-Content -NoNewline -Path (Join-Path $runtimeRoot 'logs/cli-proxy-api.err.log') -Value "smoke-error`n"
    $logsJson = & $binary '--root' $runtimeRoot '--json' 'logs' 'cli' '-n' '1'
    if ($LASTEXITCODE -ne 0 -or $logsJson -notmatch 'smoke-out' -or $logsJson -notmatch 'smoke-error' -or $logsJson -match 'old-line') {
        throw '日志读取或行数限制无效。'
    }

    & $binary '--root' $runtimeRoot 'stop' 'cli' *> $null
    if ($LASTEXITCODE -ne 0) { throw "停止临时服务标记失败，退出码：$LASTEXITCODE" }
    if (-not (Test-Path (Join-Path $runtimeRoot 'state/cli-proxy-api.disabled') -PathType Leaf)) {
        throw 'stop 未写入服务停用标记。'
    }

    $purgeCommand = '"{0}" --root "{1}" uninstall --purge < NUL > NUL 2> NUL' -f $binary, $runtimeRoot
    cmd.exe /d /s /c $purgeCommand
    if ($LASTEXITCODE -ne 2) { throw "非交互 purge 未被拒绝，退出码：$LASTEXITCODE" }
    if (-not (Test-Path $runtimeRoot -PathType Container)) { throw '被拒绝的 purge 删除了运行目录。' }
}
finally {
    if ($null -eq $previousPlatformIsolation) { Remove-Item Env:CPACTL_SMOKE_NO_PLATFORM_COMMANDS -ErrorAction SilentlyContinue }
    else { $env:CPACTL_SMOKE_NO_PLATFORM_COMMANDS = $previousPlatformIsolation }
    Remove-Item -LiteralPath $temporaryRoot -Recurse -Force -ErrorAction SilentlyContinue
}
