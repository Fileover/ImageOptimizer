#Requires -Version 7.0
<#
.SYNOPSIS
  Reports (and optionally applies) newer upstream releases for the pinned native dependencies.
.DESCRIPTION
  Never changes anything unless -Apply is given. With -Apply the tag and commit in native/dependencies.json are
  updated; submodule commits (libjxl) must be refreshed afterwards by running scripts/bootstrap.ps1 and copying the
  printed SHAs. After updating, rebuild, run all tests (scripts/build.ps1) and review CHANGELOG.md / native/PATCHES.md.
.PARAMETER Apply
  Write the newest stable tag into native/dependencies.json.
#>
[CmdletBinding()]
param([switch]$Apply)

. (Join-Path $PSScriptRoot 'common.ps1')
Assert-Command git 'Install Git.'

$path = Join-Path $Root 'native/dependencies.json'
$json = Get-Content -Raw -LiteralPath $path | ConvertFrom-Json
$changed = $false

foreach ($d in $json.native) {
    $refs = git ls-remote --tags --refs $d.repository 2>$null
    if (-not $refs) { Write-Warn "$($d.name): could not list tags (offline?)"; continue }
    $tags = foreach ($line in $refs) {
        $sha, $ref = $line -split "`t"
        $tag = $ref -replace '^refs/tags/', ''
        # stable tags only: vMAJOR.MINOR[.PATCH]
        if ($tag -match '^v?(\d+)\.(\d+)(?:\.(\d+))?$') {
            [pscustomobject]@{ Tag = $tag; Sha = $sha; Version = [version]("{0}.{1}.{2}" -f $Matches[1], $Matches[2], ($Matches[3] ?? 0)) }
        }
    }
    $latest = $tags | Sort-Object Version -Descending | Select-Object -First 1
    if (-not $latest) { Write-Warn "$($d.name): no stable tag found"; continue }
    if ($latest.Tag -eq $d.tag) { Write-Ok "$($d.name) is up to date ($($d.tag))"; continue }
    Write-Host ("    {0}: pinned {1} -> available {2} ({3})" -f $d.name, $d.tag, $latest.Tag, $latest.Sha.Substring(0, 12)) -ForegroundColor Yellow
    if ($Apply) {
        # An annotated tag points to a tag object; resolve the peeled commit.
        $peeled = (git ls-remote $d.repository "refs/tags/$($latest.Tag)^{}") -split "`t" | Select-Object -First 1
        $d.tag = $latest.Tag
        $d.commit = if ($peeled) { $peeled } else { $latest.Sha }
        $changed = $true
    }
}

if ($changed) {
    $json | ConvertTo-Json -Depth 20 | Set-Content -LiteralPath $path -Encoding utf8
    Write-Warn 'native/dependencies.json updated. Run scripts/bootstrap.ps1, refresh submodule SHAs, rebuild and re-run the tests.'
} elseif (-not $Apply) {
    Write-Host 'Dry run: nothing was changed. Use -Apply to update native/dependencies.json.'
}
Write-Host 'Rust crates: update with "cargo update" (review Cargo.lock), then run the tests.'
