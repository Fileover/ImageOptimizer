# ImageOptimizer

[![CI](https://github.com/Fileover/ImageOptimizer/actions/workflows/ci.yml/badge.svg)](https://github.com/Fileover/ImageOptimizer/actions/workflows/ci.yml)

One native Windows x64 executable (`ImageOptimizer.exe`) that integrates the core functionality of **cjxl**
(libjxl), **cwebp** (libwebp), **jpegoptim** (with MozJPEG) and **oxipng** (with Zopfli) — as statically linked
libraries, not as helper programs. No external `.exe`/`.dll`, no `Command::new`, no installer, no VC++
redistributable (static CRT).

## Design principles

* **Upstream defaults, maximum optimization.** Each codec follows the default compression mode of the upstream tool it
  is based on (`cjxl`, `cwebp`, `jpegoptim`, MozJPEG `cjpeg`, `oxipng`), paired with maximum standard compression
  effort (OxiPNG preset 6 + Zopfli, WebP method 6, JPEG XL effort 9/10, MozJPEG max-compression profile).
* **First-class `--lossless` mode.** Add `--lossless` to enforce strict mathematically lossless encoding/transcoding
  across every codec, with automatic pixel/DCT/bitstream verification and guard rails that refuse inherently lossy
  routes instead of silently downgrading them.
* **Single file or folder.** Pass a direct input/output file pair (`ImageOptimizer photo.png photo.webp` or
  `-i photo.png -o photo.jxl`) to write straight to the target path (format inferred from extension, no extra folders or
  `Reports/` created), or pass directories for parallel batch processing (`<output>/<Format>/…`).
* **Sources are never modified.** Outputs are written to a temporary file and renamed only after verification.
  Existing outputs are kept unless `--overwrite`. No `Reports/` folder is created unless `--report` is requested.
* **Metadata is preserved by default** (EXIF incl. orientation, ICC, XMP). `--strip-metadata` removes EXIF/XMP/IPTC/
  comments (ICC stays); `--strip-icc` also removes ICC.
* **Detection by file signature**, not extension: PNG (incl. APNG), JPEG, WebP, JPEG XL, BMP, TIFF, GIF.
  Animated GIF/WebP/JXL are *refused*, never flattened to the first frame without notice.
* **Every result is verified** before it is written (disable with `--verify=false`). See [CODECS.md](CODECS.md).

## Quick start

```powershell
# Single file -> single file (format inferred from output extension, no Reports/ created):
ImageOptimizer photo.png photo.webp                     # cwebp: lossy q=75, method 6
ImageOptimizer photo.png photo.webp -q 85               # custom quality (no --lossy needed)
ImageOptimizer photo.png photo.jxl --lossless           # libjxl lossless, effort 10
ImageOptimizer photo.jpg photo.jxl                      # cjxl lossless JPEG transcode, effort 10
ImageOptimizer -i photo.png -o photo.jpg                # MozJPEG q=75, progressive, trellis
ImageOptimizer photo.png optimized.png                  # OxiPNG preset 6 + Zopfli

# Batch directory processing:
ImageOptimizer --input D:\Images --format jxl      --output D:\Output
ImageOptimizer --input D:\Images --format webp     --output D:\Output
ImageOptimizer --input D:\Images --format original --lossless --recursive --output D:\Output

# Force lossy recompression even on JPEG -> JPEG / JPEG -> JXL:
ImageOptimizer photo.jpg smaller.jpg --quality 80
ImageOptimizer photo.jpg smaller.jxl --lossy --distance 1.5
```

## What happens per route

| Source → target | Default mode (upstream behavior + max effort) | `--lossless` mode (strict lossless) |
|---|---|---|
| PNG → PNG | OxiPNG preset 6 + Zopfli (`pixel-identical`, APNG kept) | Same (strict; `--optimize-alpha` disallowed) |
| any → PNG | Initial PNG + OxiPNG preset 6 + Zopfli (`pixel-identical`) | Same |
| JPEG → JPEG | `jpegoptim` lossless Huffman optimization (or `jpegoptim -m` if `--quality` given) | `jpegoptim` lossless (`dct-coefficients-identical`) |
| non-JPEG → JPEG | MozJPEG `cjpeg` profile: q=75, progressive, trellis, optimized scans | Refused (`--lossless` forbids lossy JPEG encoding) |
| JPEG → JXL | `cjxl` lossless JPEG transcode, effort 10 (`jpeg-bitstream-identical`); lossy if `--lossy`/`-q < 100`/`-d > 0` | `cjxl` lossless JPEG transcode, effort 10 (`jpeg-bitstream-identical`) |
| non-JPEG → JXL | `cjxl` visually lossless `d=1.0`, effort 9 (or lossless if `-d 0` / `-q 100`) | `libjxl` lossless, effort 10 (`pixel-identical` RGBA16) |
| any → WebP | `cwebp` lossy q=75, method 6, alpha_q=100 (optional `--sharp-yuv`) | `libwebp` lossless, q=100, method 6, `exact=1` (`pixel-identical`); 16-bit refused |
| WebP → WebP (`original`) | Lossless VP8L stays lossless unless `--lossy`/`--quality`; lossy VP8 re-encodes at q=75, method 6 | Lossless VP8L optimized losslessly; lossy VP8 refused |

* **Lossless verification:** PNG/WebP/JXL compare decoded RGBA16 samples (including hidden RGB under alpha 0);
  JPEG→JPEG compares every DCT coefficient, quantization table and marker; JPEG→JXL reconstructs the JPEG and
  checks **byte-for-byte equality**.
* **Lossy verification:** decodes and checks dimensions (`decodable-lossy-not-compared`).
* **JPEG → PNG/WebP/JXL (pixel)** in lossless mode preserves the *decoded* pixels exactly; it cannot restore quality
  the JPEG already lost.

## Options

Run `ImageOptimizer --help`. Highlights:

* **Modes:** default (upstream per-codec defaults + max effort), `--lossless` (strict lossless everywhere; conflicts
  with `--quality`, `--distance`, `--optimize-alpha`), `--lossy` (force lossy even on JPEG→JPEG / JPEG→JXL).
* **Codec tuning:** `--quality` (default: 75 WebP, 75 JPEG, mapped to distance for JXL — **not comparable between codecs**),
  `--distance` (JXL Butteraugli 0–25, default 1.0, 0 = lossless), `--effort` (JXL 1–10, default 9 lossy / 10 lossless),
  `--method` (WebP 0–6, default 6), `--alpha-quality` (WebP 0–100, default 100), `--sharp-yuv` (WebP sharp RGB→YUV),
  `--jpeg-mode keep|progressive|baseline|auto` (`auto` tests both baseline and progressive for JPEG→JPEG and keeps
  the smaller file), `--png-preset` (0–6, default 6), `--optimize-alpha` (OxiPNG `-a`), `--no-zopfli`,
  `--zopfli-iterations` (default 60), `--zopfli-no-improvement` (default 15).
* **Execution & safety:** `--jobs N` (files in parallel) × `--threads N` (threads per file) planned together against
  logical CPUs and available RAM; `--recursive`, `--overwrite`, `--dry-run`, `--keep-larger`, `--report`,
  `--max-pixels`, `--strip-metadata`, `--strip-icc`, `--verify=false`, `--config file.json` (CLI wins), `-v/-vv`,
  `--version`.

`--config` example (every key mirrors an option, unknown keys are rejected):

```json
{ "format": "webp", "quality": 80, "sharp_yuv": true, "recursive": true, "jobs": 4, "threads": 2 }
```

Exit codes: `0` ok · `1` at least one file failed · `2` invalid arguments/configuration · `130` cancelled (Ctrl+C).

## Reports (optional)

No `Reports/` folder is created by default. Pass `--report` (or `"report": true` in `--config`) to write
`<output>/Reports/report.csv` and `report.json`: per-file status, effective mode (`lossless`/`lossy`), formats, sizes,
saved bytes/%, dimensions, verification result, codec settings, notes/errors and per-stage timings (load, decode,
encode, verify, write, total).

## Building & CI

See [BUILDING.md](BUILDING.md) for local Windows x64 / Linux builds and the GitHub Actions CI workflow
(`.github/workflows/ci.yml`), which builds the statically linked `ImageOptimizer.exe`, runs the full test suite,
verifies static CRT/codec linking via `dumpbin`, and publishes build artifacts.

## Limits

* Not supported: animated output/animation re-encoding, CMYK→JPEG/WebP/JXL, arithmetic-coded JPEG output, extended XMP,
  HDR/PQ pipelines.
* Corrupt or damaged inputs are rejected (libjpeg warnings are treated as errors), never silently "repaired".
* Byte-for-byte parity with the standalone upstream CLIs is not claimed; settings map to the same underlying libraries
  and options (see [CODECS.md](CODECS.md)).

## Documentation

[BUILDING.md](BUILDING.md) · [ARCHITECTURE.md](ARCHITECTURE.md) · [CODECS.md](CODECS.md) ·
[DEPENDENCIES.md](DEPENDENCIES.md) · [LICENSES.md](LICENSES.md) · [CHANGELOG.md](CHANGELOG.md) ·
[THIRD-PARTY-NOTICES.md](THIRD-PARTY-NOTICES.md)

License: **GPL-3.0-or-later** (combined work, because jpegoptim code is included).
