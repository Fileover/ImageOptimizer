#Requires -Version 7.0
<#
.SYNOPSIS
  Builds ImageOptimizer.exe (x86_64-pc-windows-msvc, release, static CRT) and stages it in dist/.
.PARAMETER SkipTests
  Do not run the automated tests before building.
#>
[CmdletBinding()]
param([switch]$SkipTests)

. (Join-Path $PSScriptRoot 'common.ps1')

Assert-Command cargo 'Run scripts/bootstrap.ps1.'
Test-NativeSources
Enter-MsvcEnvironment

if (Get-Command ninja -ErrorAction SilentlyContinue) {
    $env:CMAKE_GENERATOR = 'Ninja'
}

if (-not $SkipTests) {
    Write-Step 'Running automated tests (cargo test --locked)'
    Invoke-Native cargo @('test', '--locked', '--release', '--target', $Target)
}

Write-Step 'Building ImageOptimizer.exe'
Invoke-Native cargo @('build', '--release', '--locked', '--target', $Target)

$exe  = Join-Path $Root "target/$Target/release/ImageOptimizer.exe"
if (-not (Test-Path $exe)) { throw "Build finished but $exe does not exist." }
$dist = Join-Path $Root 'dist'
New-Item -ItemType Directory -Force -Path $dist | Out-Null
Copy-Item $exe $dist -Force
Copy-Item (Join-Path $Root 'LICENSE') (Join-Path $dist 'LICENSE') -Force
Copy-Item (Join-Path $Root 'licenses/THIRD-PARTY-NOTICES.md') (Join-Path $dist 'THIRD-PARTY-NOTICES.md') -Force

$hash = (Get-FileHash -Algorithm SHA256 (Join-Path $dist 'ImageOptimizer.exe')).Hash
Set-Content -LiteralPath (Join-Path $dist 'ImageOptimizer.exe.sha256') -Value "$hash  ImageOptimizer.exe"
Write-Ok "dist/ImageOptimizer.exe  ($([math]::Round((Get-Item $exe).Length/1MB,2)) MiB)  SHA256 $hash"
Write-Step 'Next: scripts/verify-build.ps1 (static linking and dependency check)'
