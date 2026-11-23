#Requires -Version 7.0
<#
.SYNOPSIS
  Builds (and caches) the static native libraries: libwebp, libjxl (+brotli/highway/skcms), MozJPEG and the C shims.
.DESCRIPTION
  The native build is driven by build.rs (cmake crate) so that Cargo can cache it. This script prepares the MSVC
  environment, checks the pinned sources and compiles only the library part of the crate (no executable is linked).
  Every library is built static with the static CRT (/MT).
.PARAMETER Clean
  Remove previously built native artifacts first (cargo clean -p image-optimizer).
#>
[CmdletBinding()]
param([switch]$Clean)

. (Join-Path $PSScriptRoot 'common.ps1')

Assert-Command cargo 'Run scripts/bootstrap.ps1.'
Assert-Command cmake 'Run scripts/bootstrap.ps1.'
Test-NativeSources
Enter-MsvcEnvironment

if ($Clean) { Invoke-Native cargo @('clean', '-p', 'image-optimizer', '--release', '--target', $Target) }

Write-Step 'Building native libraries (libwebp, libjxl, MozJPEG, shims)'
$env:CMAKE_GENERATOR = 'Ninja'
if (-not (Get-Command ninja -ErrorAction SilentlyContinue)) { Remove-Item Env:CMAKE_GENERATOR }   # fall back to the VS generator
Invoke-Native cargo @('build', '--release', '--locked', '--lib', '--target', $Target)
Write-Ok 'Native libraries are built and cached by Cargo.'
