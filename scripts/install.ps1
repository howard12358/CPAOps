[CmdletBinding()]
param(
    [string]$Version,
    [string]$InstallDir = (Join-Path $env:LOCALAPPDATA 'Programs\cpactl\bin'),
    [switch]$NoInstall
)

$ErrorActionPreference = 'Stop'
$repository = 'howard12358/CPAOps'
if ([string]::IsNullOrWhiteSpace($Version)) {
    $latestUrl = (& curl.exe -fsSL -o NUL -w '%{url_effective}' "https://github.com/$repository/releases/latest").Trim()
    if ($LASTEXITCODE -ne 0 -or [string]::IsNullOrWhiteSpace($latestUrl)) {
        throw '无法确定 cpactl 最新版本。请检查网络或代理环境变量。'
    }
    $Version = ($latestUrl -split '/')[-1]
    if ($Version -notmatch '^v\d+\.\d+\.\d+(?:[-+][0-9A-Za-z.-]+)?$') {
        throw "GitHub 返回的最新版本号无效：$Version"
    }
}
$asset = "cpactl-$Version-windows-amd64.zip"
$baseUrl = "https://github.com/$repository/releases/download/$Version"
$temporary = Join-Path ([IO.Path]::GetTempPath()) ("cpactl-" + [guid]::NewGuid())
New-Item -ItemType Directory -Path $temporary | Out-Null
try {
    $archive = Join-Path $temporary $asset
    $checksums = Join-Path $temporary 'checksums.txt'
    Write-Host "下载 cpactl $Version…"
    & curl.exe -fL --retry 3 -o $archive "$baseUrl/$asset"
    & curl.exe -fL --retry 3 -o $checksums "$baseUrl/checksums.txt"
    if ($LASTEXITCODE -ne 0) { throw '下载失败。请检查网络或代理环境变量。' }
    $expected = ((Get-Content $checksums | Where-Object { $_ -match "\s$([regex]::Escape($asset))$" }) -split '\s+')[0]
    if (-not $expected -or (Get-FileHash $archive -Algorithm SHA256).Hash -ne $expected.ToUpperInvariant()) { throw 'SHA-256 校验失败。' }
    Expand-Archive -Path $archive -DestinationPath $temporary -Force
    New-Item -ItemType Directory -Force -Path $InstallDir | Out-Null
    Move-Item (Join-Path $temporary 'cpactl.exe') (Join-Path $InstallDir 'cpactl.exe') -Force
} finally { Remove-Item -Recurse -Force $temporary -ErrorAction SilentlyContinue }

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if (($userPath -split ';') -notcontains $InstallDir) {
    [Environment]::SetEnvironmentVariable('Path', (($userPath, $InstallDir | Where-Object { $_ }) -join ';'), 'User')
}
$env:Path = "$InstallDir;$env:Path"
Write-Host "已安装：$InstallDir\cpactl.exe"
if ($env:http_proxy -or $env:https_proxy -or $env:all_proxy -or $env:HTTP_PROXY -or $env:HTTPS_PROXY -or $env:ALL_PROXY) {
    & (Join-Path $InstallDir 'cpactl.exe') proxy set
}
if (-not $NoInstall -and $Host.Name -ne 'ServerRemoteHost') { & (Join-Path $InstallDir 'cpactl.exe') install }
