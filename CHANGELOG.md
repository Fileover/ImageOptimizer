# Changelog

## 0.1.0 — 2026-11-23
* First release: JPEG XL, WebP, JPEG and PNG engines linked statically (libjxl 0.13.0, libwebp 1.6.0,
  MozJPEG 4.1.5 + jpegoptim 1.5.6 core, OxiPNG 10.2.1/Zopfli).
* Running `ImageOptimizer` with no arguments prints version, linked engines, usage, and common examples.
* Canonical CLI invocation `ImageOptimizer [OPTIONS] <INPUT> <OUTPUT>` matching exact upstream tool defaults and flags:
  `cjxl` (`-d`, `-e`, `--allow_expert_options`, `--brotli_effort`, `--lossless_jpeg`; default `d=1.0, e=7, brotli=9`),
  `cwebp` (`-q`, `-m`, `--lossless-preset`, `--alpha_q`, `--exact`, `--sharp_yuv`; default `q=75, m=4`),
  `jpegoptim` (`--max`, `--all-progressive`, `--all-normal`, `--strip-all`; lossless default + smart fallback),
  MozJPEG `cjpeg` (`q=75` progressive/trellis), and `oxipng` (`--opt 0-6|max`, `-z`/`--zopfli`, `--zi`, `--ziwi`,
  `-a`/`--alpha`; default `--opt 2` libdeflate).
* Added `--max-compression` (`--best`) preset to enable maximum compression effort across all codecs in one flag.
* Downscale-only resizing (`--resize 256x256`, `256x`, `x256`, `256x256!`) with selectable resampling filter
  (`--resize-filter`, default `lanczos3`), premultiplied-alpha handling, and EXIF orientation normalization; never
  upscales and keeps lossless bitstream fast paths for images already within bounds.
* Direct single-file input/output support (positional `INPUT OUTPUT` or `-i`/`-o`) with automatic target format
  inference from the output extension, plus batch directory processing.
* `--jobs/--threads` scheduler, opt-in CSV/JSON reports (`--report`; disabled by default), per-stage timings, JSON
  config, tracing logs.
* PowerShell 7 build/verify scripts and GitHub Actions CI workflow (Windows x64 static release build + Linux test suite).
