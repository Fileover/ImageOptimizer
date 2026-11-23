#Requires -Version 7.0
# Shared helpers dot-sourced by the other scripts. Not meant to be run directly.
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'

$script:Root   = (Resolve-Path (Join-Path $PSScriptRoot '..')).Path
$script:Target = 'x86_64-pc-windows-msvc'

function Write-Step([string]$Message) { Write-Host "==> $Message" -ForegroundColor Cyan }
function Write-Ok([string]$Message)   { Write-Host "    OK  $Message" -ForegroundColor Green }
function Write-Warn([string]$Message) { Write-Host "    WARN $Message" -ForegroundColor Yellow }

function Assert-Command([string]$Name, [string]$Hint) {
    if (-not (Get-Command $Name -ErrorAction SilentlyContinue)) {
        throw "Required tool '$Name' was not found in PATH. $Hint"
    }
}

# Runs a native command and fails when the exit code is non-zero.
function Invoke-Native {
    param([Parameter(Mandatory)][string]$File, [string[]]$Arguments = @(), [string]$WorkingDirectory = $script:Root)
    Push-Location $WorkingDirectory
    try {
        & $File @Arguments
        if ($LASTEXITCODE -ne 0) { throw "'$File $($Arguments -join ' ')' failed with exit code $LASTEXITCODE" }
    } finally { Pop-Location }
}

function Get-Dependencies {
    $path = Join-Path $script:Root 'native/dependencies.json'
    return Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
}

function Find-VisualStudio {
    $vswhere = Join-Path ${env:ProgramFiles(x86)} 'Microsoft Visual Studio/Installer/vswhere.exe'
    if (-not (Test-Path $vswhere)) { return $null }
    $p = & $vswhere -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath
    if ($p) { return $p.Trim() } else { return $null }
}

# Imports the MSVC x64 environment (cl.exe, link.exe, dumpbin.exe, INCLUDE/LIB) into this session.
function Enter-MsvcEnvironment {
    if (Get-Command cl.exe -ErrorAction SilentlyContinue) { return }
    $vs = Find-VisualStudio
    if (-not $vs) { throw 'Visual Studio Build Tools with the "MSVC v143 x64/x86 build tools" component were not found. Run scripts/bootstrap.ps1.' }
    $bat = Join-Path $vs 'VC/Auxiliary/Build/vcvars64.bat'
    $dump = cmd.exe /c "`"$bat`" >nul && set"
    foreach ($line in $dump) {
        if ($line -match '^([^=]+)=(.*)$') { [Environment]::SetEnvironmentVariable($Matches[1], $Matches[2], 'Process') }
    }
    Assert-Command cl.exe 'vcvars64.bat did not provide cl.exe.'
}

function Test-NativeSources {
    $deps = Get-Dependencies
    $missing = @()
    foreach ($d in $deps.native) {
        $p = Join-Path $script:Root $d.path
        if (-not (Test-Path (Join-Path $p '.git')) -and -not (Test-Path $p)) { $missing += $d.name; continue }
        if (Get-Command git -ErrorAction SilentlyContinue) {
            $head = (git -C $p rev-parse HEAD 2>$null)
            if ($head -and $head.Trim() -ne $d.commit) { throw "$($d.name): checked-out commit $($head.Trim()) differs from the pinned $($d.commit). Run scripts/bootstrap.ps1." }
            $dirty = git -C $p status --porcelain 2>$null
            if ($dirty) { throw "$($d.name): the upstream source tree has local modifications. Patches are not allowed outside native/shims (see native/PATCHES.md)." }
        }
    }
    if ($missing) { throw "Missing native sources: $($missing -join ', '). Run scripts/bootstrap.ps1." }
}
