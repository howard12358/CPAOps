$ErrorActionPreference = 'Stop'
$script:Failures = 0
function Assert-Equal { param($Expected,$Actual,[string]$Message=''); if ($Expected -ne $Actual) { throw "Expected [$Expected], got [$Actual]. $Message" } }
function Assert-Path { param([string]$Path,[string]$Type='Any'); if (!(Test-Path $Path -PathType $Type)) { throw "Missing $Type path: $Path" } }
function Assert-Throws { param([scriptblock]$Action,[string]$Contains); try { & $Action; throw 'Expected an exception.' } catch { if ($_.Exception.Message -eq 'Expected an exception.') { throw }; if ($Contains -and $_.Exception.Message -notlike "*$Contains*") { throw "Expected error containing [$Contains], got [$($_.Exception.Message)]" } } }
function New-TestRoot { $path=Join-Path ([IO.Path]::GetTempPath()) ("cpa-stack-test-" + [guid]::NewGuid()); New-Item -ItemType Directory -Path $path | Out-Null; $path }
