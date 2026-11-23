# Codec mapping, defaults and verification

| Upstream tool | Library in this exe | Default settings (matching upstream) | `--max-compression` settings |
|---|---|---|---|
| cjxl | libjxl 0.13.0 (+ brotli, highway, skcms) | JPEG: lossless transcode (`--lossless_jpeg=1`) · other: `d=1.0`, `e=7`, `brotli_effort=9` | Lossless: `e=11` (expert) + `brotli=11` · Lossy: `e=10` + `brotli=11` |
| cwebp | libwebp 1.6.0 (+ mux/demux/sharpyuv) | Lossy `q=75`, `m=4`, `alpha_q=100`, `exact=false`, `sharp_yuv=false` | `m=6` (and `q=100` in lossless, equiv. `cwebp -z 9`) |
| jpegoptim | jpegoptim 1.5.6 core, on MozJPEG 4.1.5 | Lossless Huffman, `keep` progressive/baseline (`--max=N` for lossy ceiling) | `--jpeg-mode auto` (tries baseline & progressive, keeps smaller) |
| cjpeg (MozJPEG) | MozJPEG 4.1.5 | Lossy `q=75`, progressive, trellis, optimized scans (`JCP_MAX_COMPRESSION`) | Already uses `JCP_MAX_COMPRESSION` |
| oxipng | oxipng 10.2.1 + Zopfli | Lossless `--opt 2`, libdeflate, `alpha=false` | `--opt 6` (`max`) + Zopfli (`--zi 60 --ziwi 15`) |

> **Metadata policy:** For safety and consistency across formats, ICC, EXIF and XMP metadata are preserved by default
> in all codecs (note that upstream `cwebp` strips metadata unless `-metadata all` is given). Use `--strip-metadata`,
> `--strip-icc`, or `--strip-all` to remove metadata explicitly.

## JPEG XL (libjxl / cjxl)
* **Default mode (matches `cjxl`):**
  * **JPEG sources:** `JxlEncoderStoreJPEGMetadata` + `JxlEncoderAddJPEGFrame` (reversible lossless transcode,
    `--lossless_jpeg=1`, `effort = 7`, `brotli_effort = 9`). Verification decodes the JXL back to JPEG and requires
    **byte equality**. Passing `--lossless_jpeg=0`, `--lossy`, `-d > 0`, or `-q < 100` decodes the JPEG and re-encodes
    pixels.
  * **Non-JPEG sources:** Butteraugli `distance = 1.0` (visually lossless, `cjxl` default), `effort = 7`,
    `brotli_effort = 9`. Passing `-d 0` / `--distance=0` or `-q 100` (without `--lossy`) selects mathematically lossless
    encoding.
* **Expert options:** `--effort=11` requires `--allow_expert_options` (matching `cjxl`), which calls
  `JxlEncoderAllowExpertOptions(enc)`. `--max-compression` enables expert options automatically (`effort=11` lossless /
  `10` lossy, `brotli_effort=11`).
* **`--lossless` mode:** `JxlEncoderSetFrameLossless`, `uses_original_profile=true`; lossless JPEG transcoding for JPEG
  inputs (unless `--lossless_jpeg=0` or `--resize` downscales the image).
* EXIF/XMP stored in `Exif`/`xml ` boxes; EXIF orientation is written to the codestream header and the stored pixels are
  not rotated. Decoding for display applies orientation; decoding for verification does not.
* Animated JXL is refused. 16-bit and alpha are kept (one straight-alpha extra channel; premultiplied alpha is
  un-premultiplied on decode; other extra channels are refused).

## WebP (libwebp / cwebp)
* **Default mode (matches `cwebp`):** lossy, `quality = 75` (`-q`), `method = 4` (`-m`), `alpha_q = 100` (`--alpha_q`),
  optional `--sharp_yuv`. 16-bit sources are reduced to 8-bit with a note.
* **Lossless mode (`--lossless` or `--lossless-preset 0..9`):**
  * `--lossless`: `lossless = 1`, `quality = 75` (compression effort trade-off, `100` under `--max-compression`,
    overridable with `-q`), `method = 4` (`6` under `--max-compression`, overridable with `-m`).
  * `--lossless-preset 0..9`: calls `WebPConfigLosslessPreset` (matches `cwebp -z 0..9`).
  * `--exact`: preserves exact RGB values under fully transparent pixels (`alpha == 0`, matching `cwebp -exact`).
    Without `--exact`, libwebp optimizes hidden transparent RGB (`visible-pixels-identical`).
  * 16-bit sources and lossy WebP→WebP are refused in lossless mode.
* **`--format original` on lossless WebP (VP8L):** stays lossless by default unless `--lossy` or `-q` is passed.
* Animated WebP is refused. Dimension limit 16383×16383 enforced.

## JPEG (jpegoptim core + MozJPEG)
* **JPEG → JPEG (`jpegoptim` core on MozJPEG):**
  * **Default / `--lossless`:** lossless Huffman optimization, DCT coefficients and quantization tables untouched.
    `--jpeg-mode` defaults to `keep` (preserves input baseline/progressive), supports `progressive`
    (`--all-progressive`), `baseline` (`--all-normal`), and `auto` (tries both and keeps the smaller output; default
    under `--max-compression`). JFIF version/density preserved (local patch L-3).
  * **With `--max=N` / `-q N` or `--lossy`:** `jpegoptim --max` recompression (default quality 75 if `--lossy` without
    quality). Runs lossless optimization as well and keeps the lossless result whenever it is smaller than or equal to
    the lossy re-encode.
  * Files with data after EOI (e.g. multi-picture) and truncated/damaged files are refused.
* **Non-JPEG → JPEG (MozJPEG `cjpeg` max-compression profile):**
  * **Default / `--lossy`:** quality 75 default (`-q`), progressive by default (override `--all-normal`), trellis
    quantization and optimized scan scripts. Alpha is composited on white (reported), 16-bit reduced to 8-bit
    (reported), CMYK sources refused.
  * **`--lossless`:** refused because JPEG is an inherently lossy codec.

## PNG (OxiPNG + Zopfli)
* Always lossless, matching upstream `oxipng` defaults: `--opt 2` (libdeflate), `alpha = false` (hidden RGB under
  alpha 0 untouched), `scale_16 = false`.
* Pass `--opt 6` / `--opt max`, `-z`/`--zopfli` (`--zi` default 15, `--ziwi` optional), or `--max-compression`
  (`--opt 6 -z --zi 60 --ziwi 15`) for maximum compression.
* Pass `-a`/`--alpha` (`oxipng -a`) for visually lossless optimization of transparent RGB (disallowed under
  `--lossless`).
* APNG animation is preserved in PNG→PNG optimization (unless downscaled via `--resize`); an APNG cannot be a
  *conversion* source.

## BMP / TIFF / GIF
Decoded with the `image` crate (single-frame only). They have no "optimize in place" path: choose an output file
extension (`.png`, `.jpg`, `.webp`, `.jxl`) or `--format png|webp|jpeg|jxl`.

## Verification matrix

| Route | Check |
|---|---|
| PNG (default), JXL lossless, WebP lossless (`--exact`) | RGBA16 equality (`pixel-identical`) + metadata equality for PNG→PNG |
| PNG (`--alpha`), WebP lossless (without `--exact`) | Alpha channel exact + RGB exact wherever alpha > 0 (`visible-pixels-identical`) |
| JPEG→JPEG lossless (incl. `jpegoptim -m` lossless fallback) | All DCT coefficients, tables, dimensions, saved markers (`dct-coefficients-identical`) |
| JPEG→JXL lossless transcode | Reconstruct JPEG from JXL, byte-equal (`jpeg-bitstream-identical`) |
| Any lossy route | Decodes cleanly, same dimensions (`decodable-lossy-not-compared`) |
| Kept original | `byte-identical-copy` |
