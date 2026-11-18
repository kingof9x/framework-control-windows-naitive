<#
.SYNOPSIS
  Builds the Framework Control (native) Windows installer.

.DESCRIPTION
  1. Builds the background service (without the embedded web UI) with this
     fork's release settings baked in, so its updater follows this repo's
     GitHub releases.
  2. Builds the native desktop app.
  3. Packs both, plus WinSW, into an MSI with WiX Toolset v3.

  Output: native\dist\FrameworkControl-<version>-x64.msi
          native\dist\framework-control-app-<version>-x64.zip (app only)

  Requirements: Rust (stable, MSVC), WiX Toolset v3 (`choco install wixtoolset`).

.EXAMPLE
  pwsh native/installer/build-msi.ps1
  pwsh native/installer/build-msi.ps1 -Port 8091
#>
param(
    [int]$Port = 8090,
    [string]$UpdateRepo = 'kingof9x/framework-control-windows-naitive'
)

$ErrorActionPreference = 'Stop'
$root = Resolve-Path (Join-Path $PSScriptRoot '..\..')
$native = Join-Path $root 'native'
$service = Join-Path $root 'service'
$dist = Join-Path $native 'dist'
$stage = Join-Path $native 'target\msi-stage'

function Step($msg) { Write-Host "`n==> $msg" -ForegroundColor Cyan }
function Run($exe, [string[]]$argv) {
    & $exe @argv
    if ($LASTEXITCODE -ne 0) { throw "$exe $($argv -join ' ') failed with exit code $LASTEXITCODE" }
}

# Release version = the native app's version (one version for the whole MSI).
$version = (Select-String -Path (Join-Path $native 'Cargo.toml') -Pattern '^version\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value
Write-Host "Framework Control $version (port $Port, updates from $UpdateRepo)"

# WiX location: $env:WIX (set by the installer) or PATH.
$candle = if ($env:WIX -and (Test-Path "$env:WIX\bin\candle.exe")) { "$env:WIX\bin\candle.exe" } else { (Get-Command candle.exe -ErrorAction SilentlyContinue).Source }
$light = if ($env:WIX -and (Test-Path "$env:WIX\bin\light.exe")) { "$env:WIX\bin\light.exe" } else { (Get-Command light.exe -ErrorAction SilentlyContinue).Source }
if (-not $candle -or -not $light) { throw 'WiX Toolset v3 not found. Install it with: choco install wixtoolset' }

Step 'Building service'
$env:FRAMEWORK_CONTROL_PORT = "$Port"
$env:FRAMEWORK_CONTROL_ALLOWED_ORIGINS = ''
$env:FRAMEWORK_CONTROL_UPDATE_REPO = $UpdateRepo
$env:FRAMEWORK_CONTROL_RELEASE_VERSION = $version
Push-Location $service
try { Run cargo @('build', '--release', '--no-default-features') } finally { Pop-Location }

Step 'Building native app'
Push-Location $native
try { Run cargo @('build', '--release') } finally { Pop-Location }

Step 'Staging files'
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage, $dist | Out-Null
Copy-Item (Join-Path $service 'target\release\framework-control-service.exe') $stage
Copy-Item (Join-Path $native 'target\release\framework-control-app.exe') $stage
Copy-Item (Join-Path $service 'wix\FrameworkControlService.exe') $stage
Copy-Item (Join-Path $native 'assets\app.ico') $stage
# Same token replacement as upstream's build-msi.mjs.
(Get-Content -Raw (Join-Path $service 'wix\FrameworkControlService.xml')).
    Replace('@FRAMEWORK_CONTROL_ALLOWED_ORIGINS@', '').
    Replace('@FRAMEWORK_CONTROL_PORT@', "$Port").
    Replace('@FRAMEWORK_CONTROL_UPDATE_REPO@', $UpdateRepo) |
    Set-Content -NoNewline -Encoding UTF8 (Join-Path $stage 'FrameworkControlService.xml')

Step 'Building MSI'
$obj = Join-Path $native 'target\FrameworkControl.wixobj'
$msi = Join-Path $dist "FrameworkControl-$version-x64.msi"
Run $candle @('-nologo', '-arch', 'x64', "-dVersion=$version", "-dStage=$stage",
    '-ext', 'WixUIExtension', '-ext', 'WixUtilExtension',
    '-out', $obj, (Join-Path $PSScriptRoot 'FrameworkControl.wxs'))
Run $light @('-nologo', '-ext', 'WixUIExtension', '-ext', 'WixUtilExtension',
    '-spdb', '-out', $msi, $obj)

Step 'Packing portable app zip'
$zip = Join-Path $dist "framework-control-app-$version-x64.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path (Join-Path $stage 'framework-control-app.exe') -DestinationPath $zip

Get-ChildItem $dist | Format-Table Name, @{ n = 'MB'; e = { [math]::Round($_.Length / 1MB, 2) } }
if ($env:GITHUB_OUTPUT) { "version=$version" | Out-File -Append -Encoding utf8 $env:GITHUB_OUTPUT }
