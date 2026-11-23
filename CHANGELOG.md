# Changelog

## 0.1.0 — unreleased
* First implementation: JPEG XL, WebP, JPEG and PNG engines linked statically (libjxl 0.13.0, libwebp 1.6.0,
  MozJPEG 4.1.5 + jpegoptim 1.5.6 core, OxiPNG 10.2.1/Zopfli).
* Upstream-aligned per-codec defaults tuned for maximum compression effort (`cjxl`, `cwebp`, `jpegoptim`, MozJPEG
  `cjpeg`, `oxipng`), plus strict `--lossless` mode across all codecs and `--lossy` override.
* `jpegoptim --max` smart lossless fallback for JPEG→JPEG when `--quality` is specified, `--jpeg-mode auto` (baseline
  vs progressive comparison), `--sharp-yuv` for lossy WebP, and `--optimize-alpha` for PNG.
* Metadata preserved by default across all codecs (`--strip-metadata` / `--strip-icc`); magic-byte detection;
  verification matrix; atomic writes.
* Direct single-file input/output support (positional `INPUT OUTPUT` or `-i`/`-o`) with automatic target format
  inference from the output extension, plus batch directory processing.
* `--jobs/--threads` scheduler, opt-in CSV/JSON reports (`--report`; disabled by default), per-stage timings, JSON
  config, tracing logs.
* PowerShell 7 build/verify scripts and GitHub Actions CI workflow (Windows x64 static release build + Linux test suite).
