# ImageOptimizer

[![CI](https://github.com/Fileover/ImageOptimizer/actions/workflows/ci.yml/badge.svg)](https://github.com/Fileover/ImageOptimizer/actions/workflows/ci.yml)

One native Windows x64 executable (`ImageOptimizer.exe`) that integrates the core functionality of **cjxl**
(libjxl), **cwebp** (libwebp), **jpegoptim** (with MozJPEG) and **oxipng** (with Zopfli) — as statically linked
libraries, not as helper programs. No external `.exe`/`.dll`, no `Command::new`, no installer, no VC++
redistributable (static CRT).

## Design principles

* **Upstream defaults & flags.** Every output format follows the default behavior and command-line flags of the
  upstream tool it wraps (`cjxl`, `cwebp`, `jpegoptim`, MozJPEG `cjpeg`, `oxipng`), so existing upstream commands map
  directly to `ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>`.
* **First-class `--lossless` & `--max-compression`.** Use `--lossless` for strict mathematically lossless output across
  codecs and `--max-compression` to push every codec to maximum compression effort in one flag.
* **Single file or folder.** Pass `ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>` to write directly to the output file
  (format inferred from extension, no extra folders or `Reports/` created), or pass directories for parallel batch
  processing (`<output>/<Format>/…`).
* **Sources are never modified.** Outputs are written to a temporary file and renamed only after verification.
  Existing outputs are kept unless `--overwrite`. No `Reports/` folder is created unless `--report` is requested.
* **Metadata is preserved by default** (EXIF incl. orientation, ICC, XMP). `--strip-metadata` removes EXIF/XMP/IPTC/
  comments (ICC stays); `--strip-icc` also removes ICC; `--strip-all` removes all metadata including ICC.
* **Detection by file signature**, not extension: PNG (incl. APNG), JPEG, WebP, JPEG XL, BMP, TIFF, GIF.
  Animated GIF/WebP/JXL are *refused*, never flattened to the first frame without notice.
* **Every result is verified** before it is written (disable with `--verify=false`). See [CODECS.md](CODECS.md).

## Quick start

Canonical syntax: `ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>`

```powershell
# JPEG XL (cjxl defaults: d=1.0, e=7, brotli_effort=9; lossless JPEG transcode for JPEGs):
ImageOptimizer test.png out.jxl
ImageOptimizer test.jpg out.jxl
ImageOptimizer --distance=0 --effort=11 --allow_expert_options --brotli_effort=11 test.png out.jxl
ImageOptimizer --lossless --max-compression test.png out.jxl

# WebP (cwebp defaults: lossy q=75, m=4, alpha_q=100):
ImageOptimizer test.png out.webp
ImageOptimizer -q 80 -m 6 --sharp_yuv test.png out.webp
ImageOptimizer --lossless -q 100 -m 6 --exact test.png out.webp
ImageOptimizer --lossless-preset 9 test.png out.webp

# PNG (oxipng defaults: lossless --opt 2, libdeflate):
ImageOptimizer test.png out.png
ImageOptimizer --opt max -z test.png out.png
ImageOptimizer --max-compression test.png out.png

# JPEG (jpegoptim lossless default for JPEGs; MozJPEG cjpeg q=75 progressive/trellis for conversions):
ImageOptimizer test.jpg out.jpg
ImageOptimizer --max=85 --all-progressive --strip-all test.jpg out.jpg
ImageOptimizer test.png out.jpg

# Downscale-only resize (512x512 -> 256x256, never upscales):
ImageOptimizer --resize 256 --lossless --max-compression test.png thumb.jxl

# Batch directory processing:
ImageOptimizer --format jxl --recursive --input D:\Images --output D:\Output
ImageOptimizer --format original --lossless --max-compression --recursive --input D:\Images --output D:\Output
```

## Defaults per format (matching upstream tools)

| Source → target | Default mode (upstream tool behavior) | `--lossless` mode | With `--max-compression` |
|---|---|---|---|
| JPEG → JXL (`cjxl`) | Lossless JPEG transcode (`--lossless_jpeg=1`), `e=7`, `brotli=9` (`jpeg-bitstream-identical`) | Same (`jpeg-bitstream-identical`) | `e=11` (expert) + `brotli=11` |
| non-JPEG → JXL (`cjxl`) | Visually lossless `d=1.0`, `e=7`, `brotli=9` (`-d 0` / `-q 100` → lossless) | Mathematically lossless `d=0`, `e=7`, `brotli=9` (`pixel-identical`) | Lossy: `e=10` · Lossless: `e=11` (expert) + `brotli=11` |
| any → WebP (`cwebp`) | Lossy `q=75`, `m=4`, `alpha_q=100` (`--sharp_yuv` optional) | Lossless `q=75`, `m=4` (`--exact` preserves alpha=0 RGB; 16-bit refused) | `m=6` (and `q=100` in lossless, equiv. `cwebp -z 9`) |
| WebP → WebP (`original`) | Lossless VP8L stays lossless unless `--lossy`/`-q`; lossy VP8 re-encodes at `q=75, m=4` | Lossless VP8L optimized losslessly; lossy VP8 refused | Same + `m=6` / `q=100` |
| JPEG → JPEG (`jpegoptim`) | Lossless Huffman (`keep` progressive/baseline); `--max=N`/`-q N` uses `jpegoptim -m` smart fallback | Lossless (`dct-coefficients-identical`) | `--jpeg-mode auto` (tries baseline & progressive, keeps smaller) |
| non-JPEG → JPEG (`MozJPEG`) | Lossy `q=75`, progressive, trellis, optimized scans (`cjpeg` profile) | Refused (JPEG is lossy) | MozJPEG max-compression profile (already default) |
| PNG / any → PNG (`oxipng`) | Lossless `--opt 2`, libdeflate, `alpha=false` (APNG preserved for PNG→PNG) | Same (`--alpha` disallowed) | `--opt 6` + Zopfli (`--zi 60 --ziwi 15`) |

* **Lossless verification:** PNG/JXL and WebP (`--exact`) compare decoded RGBA16 samples including hidden RGB under
  alpha 0 (`pixel-identical`); WebP lossless without `--exact` and PNG with `--alpha` verify alpha and every visible
  pixel (`visible-pixels-identical`); JPEG→JPEG verifies every DCT coefficient, quantization table and marker
  (`dct-coefficients-identical`); JPEG→JXL reconstructs the JPEG and checks byte-for-byte equality
  (`jpeg-bitstream-identical`).
* **Lossy verification:** decodes and checks dimensions (`decodable-lossy-not-compared`).

## Options

Run `ImageOptimizer --help` for all grouped options (`JPEG XL (cjxl)`, `WebP (cwebp)`, `JPEG (jpegoptim / MozJPEG)`,
`PNG (oxipng)`, `Resize`, `Metadata & Execution`). Key flags:

* **Modes:** `--lossless`, `--lossy`, `--max-compression` (alias `--best`).
* **JPEG XL (`cjxl`):** `-d`/`--distance <0-25>`, `-q`/`--quality <0-100>`, `-e`/`--effort <1-11>`,
  `--allow_expert_options` (required for `-e 11`, unless `--max-compression`), `--brotli_effort <0-11>`,
  `--lossless_jpeg <0|1>`.
* **WebP (`cwebp`):** `-q`/`--quality <0-100>`, `-m`/`--method <0-6>`, `--lossless-preset <0-9>` (`cwebp -z`),
  `--alpha_q`/`--alpha-quality <0-100>`, `--exact`, `--sharp_yuv`.
* **JPEG (`jpegoptim` / MozJPEG):** `-q`/`--quality`/`--max <0-100>`, `--jpeg-mode keep|progressive|baseline|auto`,
  `--all-progressive`, `--all-normal`.
* **PNG (`oxipng`):** `--opt <0-6|max>` (alias `--png-preset`), `-z`/`--zopfli`, `--no-zopfli`, `--zi <N>`,
  `--ziwi <N>`, `-a`/`--alpha` (alias `--optimize-alpha`).
* **Downscale-only resize:** `--resize <SPEC>` (`256` or `256x256` fits inside 256×256 preserving aspect ratio; `256x`
  clamps width; `x256` clamps height; `256x256!` clamps each axis independently — never upscales), `--resize-filter`
  (`lanczos3` default, `catmull-rom`, `triangle`, `gaussian`, `nearest`).
* **Metadata & execution:** `--strip-metadata`, `--strip-icc`, `--strip-all`, `-j`/`--jobs`, `-t`/`--threads`,
  `-r`/`--recursive`, `--overwrite`, `--dry-run`, `--keep-larger`, `--report`, `--verify=false`, `--max-pixels`,
  `--config file.json` (CLI wins), `--quiet`, `-v`/`-vv`, `--list-formats`, `-V`/`--version`.

`--config` example (every key mirrors an option, unknown keys are rejected):

```json
{ "format": "webp", "quality": 80, "method": 6, "sharp_yuv": true, "recursive": true, "jobs": 4, "threads": 2 }
```

Exit codes: `0` ok · `1` at least one file failed · `2` invalid arguments/configuration · `130` cancelled (Ctrl+C).

## Reports (optional)

No `Reports/` folder is created by default. Pass `--report` (or `"report": true` in `--config`) to write
`<output>/Reports/report.csv` and `report.json` with per-file status, effective mode, formats, sizes, saved bytes/%,
dimensions, verification result, codec settings, notes/errors and per-stage timings.

## Building & CI

See [BUILDING.md](BUILDING.md) for local Windows x64 / Linux builds and the GitHub Actions CI workflow
(`.github/workflows/ci.yml`), which builds the statically linked `ImageOptimizer.exe`, runs the full test suite,
verifies static CRT/codec linking via `dumpbin`, and publishes build artifacts.

## Limits

* Not supported: animated output/animation re-encoding, CMYK→JPEG/WebP/JXL, arithmetic-coded JPEG output, extended XMP,
  HDR/PQ pipelines.
* Corrupt or damaged inputs are rejected (libjpeg warnings are treated as errors), never silently "repaired".

## Documentation

[BUILDING.md](BUILDING.md) · [ARCHITECTURE.md](ARCHITECTURE.md) · [CODECS.md](CODECS.md) ·
[DEPENDENCIES.md](DEPENDENCIES.md) · [LICENSES.md](LICENSES.md) · [CHANGELOG.md](CHANGELOG.md) ·
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)

License: **GPL-3.0-or-later** (combined work, because jpegoptim code is included).
