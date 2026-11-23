#Requires -Version 7.0
<#
.SYNOPSIS
  Prepares a Windows x64 machine to build ImageOptimizer and fetches the pinned upstream sources.
.DESCRIPTION
  * checks/installs (via winget, only with -InstallTools) Git, CMake, NASM, rustup and the VS 2022 Build Tools
  * installs the Rust toolchain from rust-toolchain.toml and the x86_64-pc-windows-msvc target
  * clones every dependency of native/dependencies.json at the pinned tag and verifies the pinned commit
  Network access is only needed here; build.ps1 works offline afterwards.
.PARAMETER InstallTools
  Install missing prerequisites with winget instead of just reporting them.
#>
[CmdletBinding()]
param([switch]$InstallTools)

. (Join-Path $PSScriptRoot 'common.ps1')

function Install-IfMissing([string]$Command, [string]$WingetId) {
    if (Get-Command $Command -ErrorAction SilentlyContinue) { Write-Ok "$Command found"; return }
    if (-not $InstallTools) { throw "$Command is missing. Re-run with -InstallTools or install '$WingetId' manually." }
    Assert-Command winget 'winget (App Installer) is required for -InstallTools.'
    Write-Step "Installing $WingetId"
    Invoke-Native winget @('install', '--id', $WingetId, '-e', '--accept-package-agreements', '--accept-source-agreements')
    Write-Warn "$WingetId was installed: open a NEW PowerShell window so PATH is refreshed, then re-run bootstrap.ps1."
    exit 3
}

Write-Step 'Checking prerequisites'
Install-IfMissing git   'Git.Git'
Install-IfMissing cmake 'Kitware.CMake'
if (-not (Get-Command nasm -ErrorAction SilentlyContinue)) {
    Write-Warn 'NASM not found: MozJPEG will be built without SIMD (slower JPEG coding, identical output). Install with: winget install NASM.NASM'
}
Install-IfMissing rustup 'Rustlang.Rustup'

if (-not (Find-VisualStudio)) {
    if ($InstallTools) {
        Assert-Command winget 'winget is required for -InstallTools.'
        Write-Step 'Installing Visual Studio 2022 Build Tools (C++ workload)'
        Invoke-Native winget @('install', '--id', 'Microsoft.VisualStudio.2022.BuildTools', '-e', '--override',
            '--quiet --wait --add Microsoft.VisualStudio.Workload.VCTools --includeRecommended')
    } else {
        throw 'Visual Studio 2022 Build Tools with the C++ workload are missing. Re-run with -InstallTools or install them manually.'
    }
}
Write-Ok 'MSVC toolchain found'

Write-Step 'Installing the pinned Rust toolchain'
Invoke-Native rustup @('toolchain', 'install', (Select-String -Path (Join-Path $Root 'rust-toolchain.toml') -Pattern 'channel\s*=\s*"([^"]+)"').Matches[0].Groups[1].Value, '--profile', 'minimal')
Invoke-Native rustup @('target', 'add', $Target)

Write-Step 'Fetching pinned native sources'
$deps = Get-Dependencies
foreach ($d in $deps.native) {
    $dest = Join-Path $Root $d.path
    if (Test-Path (Join-Path $dest '.git')) {
        $head = (git -C $dest rev-parse HEAD).Trim()
        if ($head -eq $d.commit) { Write-Ok "$($d.name) $($d.tag) already present"; continue }
        Write-Warn "$($d.name): at $head, expected $($d.commit); re-fetching"
        Invoke-Native git @('-C', $dest, 'fetch', '--depth', '1', 'origin', 'tag', $d.tag)
    } else {
        New-Item -ItemType Directory -Force -Path (Split-Path $dest) | Out-Null
        Invoke-Native git @('clone', '--depth', '1', '--branch', $d.tag, $d.repository, $dest)
    }
    Invoke-Native git @('-C', $dest, 'checkout', '--detach', $d.commit)
    if ($d.PSObject.Properties.Name -contains 'submodules') {
        foreach ($s in $d.submodules) {
            Invoke-Native git @('-C', $dest, 'submodule', 'update', '--init', '--depth', '1', '--', $s.path)
            $sh = (git -C (Join-Path $dest $s.path) rev-parse HEAD).Trim()
            if ($sh -ne $s.commit) { throw "$($d.name)/$($s.path): commit $sh differs from the pinned $($s.commit)" }
        }
    }
    Write-Ok "$($d.name) $($d.tag) @ $($d.commit.Substring(0,12))"
}

Test-NativeSources
Write-Step 'Bootstrap complete. Next: scripts/build.ps1'
