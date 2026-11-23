#Requires -Version 7.0
<#
.SYNOPSIS
  Verifies that dist/ImageOptimizer.exe is a self-contained Windows x64 executable.
.DESCRIPTION
  Checks (any failure -> non-zero exit code):
   1. PE header: machine type x64 (0x8664)
   2. dumpbin /dependents: only Windows system DLLs; no VC++ runtime / UCRT redistributable DLLs and no codec DLLs
   3. no 'Command::new' / process-spawn calls in src/ (all codecs are linked in, nothing is launched)
   4. the executable starts and reports every codec version (--version, --list-formats)
   5. a smoke test converts generated images in default and --lossless modes into every format and verifies the reports
  Item 4/5 run the executable; run this script also on a clean Windows machine without Visual Studio / VC++ runtime
  (copy only dist/ImageOptimizer.exe and use -ExePath) to prove the "no installation required" claim.
.PARAMETER ExePath
  Executable to verify (default: dist/ImageOptimizer.exe).
#>
[CmdletBinding()]
param([string]$ExePath = (Join-Path (Resolve-Path (Join-Path $PSScriptRoot '..')).Path 'dist/ImageOptimizer.exe'))

. (Join-Path $PSScriptRoot 'common.ps1')
$failed = $false
function Fail([string]$m) { Write-Host "    FAIL $m" -ForegroundColor Red; $script:failed = $true }

if (-not (Test-Path $ExePath)) { throw "Executable not found: $ExePath (run scripts/build.ps1)" }
Write-Step "Verifying $ExePath"

# 1. PE machine type
$fs = [IO.File]::OpenRead($ExePath)
try {
    $br = [IO.BinaryReader]::new($fs)
    $fs.Seek(0x3C, 'Begin') | Out-Null
    $peOffset = $br.ReadInt32()
    $fs.Seek($peOffset, 'Begin') | Out-Null
    $sig = $br.ReadUInt32()
    $machine = $br.ReadUInt16()
} finally { $fs.Dispose() }
if ($sig -ne 0x4550) { Fail 'not a PE file' }
elseif ($machine -ne 0x8664) { Fail ("machine type is 0x{0:X4}, expected 0x8664 (x64)" -f $machine) }
else { Write-Ok 'PE machine type: x64' }

# 2. imported DLLs
$allowed = @(
    'kernel32.dll','kernelbase.dll','ntdll.dll','advapi32.dll','bcrypt.dll','bcryptprimitives.dll',
    'user32.dll','shell32.dll','shlwapi.dll','ole32.dll','oleaut32.dll','rpcrt4.dll',
    'ws2_32.dll','userenv.dll','secur32.dll','crypt32.dll','ncrypt.dll','synchronization.dll',
    'iphlpapi.dll','netapi32.dll','psapi.dll','powrprof.dll','pdh.dll','wbemuuid.dll',
    'ntmarta.dll','propsys.dll','gdi32.dll','cfgmgr32.dll','setupapi.dll','version.dll','winmm.dll'
)
$forbidden = '^(vcruntime|msvcp|vcomp|concrt|msvcr|ucrtbase|api-ms-win-crt|libjxl|libwebp|libjpeg|jpeg|webp|jxl|libsharpyuv|brotli|hwy|libgcc|libstdc|libwinpthread)'
try {
    Enter-MsvcEnvironment
    $out = & dumpbin.exe /nologo /dependents $ExePath
    $dlls = $out | Where-Object { $_ -match '^\s+\S+\.dll\s*$' } | ForEach-Object { $_.Trim().ToLowerInvariant() }
    Write-Host ('    imports: ' + ($dlls -join ', '))
    foreach ($d in $dlls) {
        if ($d -match $forbidden) { Fail "forbidden runtime/codec dependency: $d" }
        elseif ($allowed -notcontains $d -and $d -notmatch '^api-ms-win-(core|eventing|security|shcore|service)') { Fail "unexpected dependency (review): $d" }
    }
    if (-not $failed) { Write-Ok 'only Windows system DLLs are imported (static CRT, static codecs)' }
} catch { Fail "dumpbin check could not run: $($_.Exception.Message)" }

# 3. no process spawning in the sources
$hits = Get-ChildItem (Join-Path $Root 'src') -Recurse -Filter *.rs |
        Select-String -Pattern 'Command::new|std::process::Command|CreateProcess|ShellExecute' |
        Where-Object { $_.Line -notmatch '^\s*//' }
if ($hits) { $hits | ForEach-Object { Fail "process spawning in $($_.Path):$($_.LineNumber)" } } else { Write-Ok 'no external process is launched by the sources' }

# 4. --version and --list-formats
$v = & $ExePath --version
if ($LASTEXITCODE -ne 0) { Fail '--version failed' }
foreach ($needle in 'libjxl', 'libwebp', 'mozjpeg', 'jpegoptim', 'oxipng') { if (($v -join "`n") -notmatch $needle) { Fail "--version does not list $needle" } }
Write-Host ($v | ForEach-Object { "    $_" } | Out-String).TrimEnd()

$lf = & $ExePath --list-formats
if ($LASTEXITCODE -ne 0 -or ($lf -join "`n") -notmatch 'JPEG XL') { Fail '--list-formats failed' }

# 5. smoke test with a pure-PowerShell generated 24-bit BMP (no System.Drawing dependency)
function Write-TestBmp([string]$Path, [int]$Width, [int]$Height) {
    $rowStride = [int]([math]::Ceiling(($Width * 3) / 4) * 4)
    $pixelSize = $rowStride * $Height
    $fileSize = 54 + $pixelSize
    $ms = [IO.MemoryStream]::new($fileSize)
    $bw = [IO.BinaryWriter]::new($ms)
    try {
        # BITMAPFILEHEADER (14 bytes)
        $bw.Write([byte[]]@(0x42, 0x4D))        # "BM"
        $bw.Write([uint32]$fileSize)
        $bw.Write([uint16]0)
        $bw.Write([uint16]0)
        $bw.Write([uint32]54)                   # pixel offset
        # BITMAPINFOHEADER (40 bytes)
        $bw.Write([uint32]40)
        $bw.Write([int32]$Width)
        $bw.Write([int32]$Height)
        $bw.Write([uint16]1)                    # planes
        $bw.Write([uint16]24)                   # bpp
        $bw.Write([uint32]0)                    # BI_RGB
        $bw.Write([uint32]$pixelSize)
        $bw.Write([int32]2835)                  # 72 dpi
        $bw.Write([int32]2835)
        $bw.Write([uint32]0)
        $bw.Write([uint32]0)
        # Pixel data (BGR bottom-up)
        $pad = $rowStride - ($Width * 3)
        for ($y = 0; $y -lt $Height; $y++) {
            for ($x = 0; $x -lt $Width; $x++) {
                $bw.Write([byte](($x * $y) % 256))
                $bw.Write([byte](($y * 4) % 256))
                $bw.Write([byte](($x * 2) % 256))
            }
            for ($p = 0; $p -lt $pad; $p++) { $bw.Write([byte]0) }
        }
        [IO.File]::WriteAllBytes($Path, $ms.ToArray())
    } finally { $bw.Dispose(); $ms.Dispose() }
}

$work = Join-Path ([IO.Path]::GetTempPath()) ("imgopt-verify-" + [guid]::NewGuid().ToString('N'))
$seed = Join-Path $work 'seed'; $in = Join-Path $work 'in'; $single = Join-Path $work 'single'; $outRoot = Join-Path $work 'out'
New-Item -ItemType Directory -Force -Path $seed, $in, $single | Out-Null
try {
    $bmpPath = Join-Path $seed 'sample.bmp'
    Write-TestBmp $bmpPath 96 64
    # Single-file mode (options-before-paths, inferred output format, no Reports/ created)
    $aPng = Join-Path $in 'a.png'
    $bJpg = Join-Path $in 'b.jpg'
    & $ExePath --quiet --opt 1 $bmpPath $aPng
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $aPng)) { Fail 'single-file BMP -> PNG failed' }
    & $ExePath --quiet $bmpPath $bJpg
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path $bJpg)) { Fail 'single-file BMP -> JPEG failed' }
    & $ExePath --quiet $aPng (Join-Path $single 'out.webp')
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $single 'out.webp'))) { Fail 'single-file PNG -> WebP failed' }
    & $ExePath --quiet --distance=0 --effort=3 $aPng (Join-Path $single 'out.jxl')
    if ($LASTEXITCODE -ne 0 -or -not (Test-Path (Join-Path $single 'out.jxl'))) { Fail 'single-file PNG -> JXL lossless failed' }
    if ((Test-Path (Join-Path $in 'Reports')) -or (Test-Path (Join-Path $single 'Reports'))) { Fail 'single-file mode unexpectedly created Reports/' }
    else { Write-Ok 'single-file input -> output ok (no Reports/ created)' }

    # Directory mode (default: no Reports/; --report: writes report.json)
    foreach ($fmt in 'jxl', 'webp', 'jpeg', 'png', 'original') {
        $o = Join-Path $outRoot "default-$fmt"
        & $ExePath --input $in --output $o --format $fmt --quiet --no-zopfli --png-preset 1 --effort 3
        if ($LASTEXITCODE -ne 0) { Fail "default '$fmt' returned exit code $LASTEXITCODE"; continue }
        if (Test-Path (Join-Path $o 'Reports')) { Fail "default '$fmt' unexpectedly created Reports/ without --report" }
        else { Write-Ok "default '$fmt': ok (no Reports/)" }
    }
    foreach ($fmt in 'jxl', 'webp', 'png', 'original') {
        $o = Join-Path $outRoot "lossless-$fmt"
        & $ExePath --input $in --output $o --format $fmt --lossless --quiet --no-zopfli --png-preset 1 --effort 3 --report
        if ($LASTEXITCODE -ne 0) { Fail "lossless '$fmt' returned exit code $LASTEXITCODE"; continue }
        $rep = Get-Content -Raw (Join-Path $o 'Reports/report.json') | ConvertFrom-Json
        if ($rep.summary.failed -ne 0) { Fail "lossless '$fmt': $($rep.summary.failed) file(s) failed" }
        else { Write-Ok "lossless '$fmt': $($rep.summary.written + $rep.summary.kept_original) file(s) ok (verified, --report ok)" }
    }
} finally { Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue }

if ($failed) { Write-Host 'VERIFICATION FAILED' -ForegroundColor Red; exit 1 }
Write-Host 'VERIFICATION PASSED' -ForegroundColor Green
