# Codec mapping, defaults and verification

| Upstream tool | Library in this exe | Upstream default mode | Max-optimization defaults here |
|---|---|---|---|
| cjxl | libjxl 0.13.0 (+ brotli, highway, skcms) | JPEG: lossless transcode · other: lossy `d=1.0` | effort 11 (lossless, expert options) / effort 9 (lossy), `brotli_effort=11` |
| cwebp | libwebp 1.6.0 (+ mux/demux/sharpyuv) | lossy `q=75`, `alpha_q=100` | method 6 (`-m 6`); `--lossless` → `q=100, m=6, exact=1` |
| jpegoptim | jpegoptim 1.5.6 core, on MozJPEG 4.1.5 | lossless Huffman optimization (`-m` for lossy max quality) | MozJPEG DC scan opt; `--jpeg-mode auto` tries baseline & progressive |
| cjpeg (MozJPEG) | MozJPEG 4.1.5 | lossy `q=75`, progressive, trellis | `JCP_MAX_COMPRESSION` profile (trellis, optimized scans, `optimize_coding`) |
| oxipng | oxipng 10.2.1 + Zopfli | lossless (`optimize_alpha=false`) | preset 6 (`max_compression`) + Zopfli (60 iter, stop after 15) |

## JPEG XL (libjxl / cjxl)
* **Default mode (matches `cjxl`):**
  * **JPEG sources:** `JxlEncoderStoreJPEGMetadata` + `JxlEncoderAddJPEGFrame` (reversible lossless transcode, effort 11
    via `JxlEncoderAllowExpertOptions`, `brotli_effort=11`). Verification decodes the JXL back to JPEG and requires
    **byte equality**. Passing `--lossy`, `--distance > 0`, or `--quality < 100` switches JPEG sources to decoded pixel
    re-encoding.
  * **Non-JPEG sources:** Butteraugli `distance = 1.0` (visually lossless, `cjxl` default), effort 9 (override with
    `--effort 1..11`), `brotli_effort=11` (override with `--brotli-effort 0..11`). Passing `--distance 0` or
    `--quality 100` (without `--lossy`) selects mathematically lossless encoding at effort 11, matching
    `cjxl --distance=0 --effort=11 --allow_expert_options --brotli_effort=11`.
* **`--lossless` mode:** `JxlEncoderSetFrameLossless`, original color profile kept (`uses_original_profile=true`),
  effort 11 (with `JxlEncoderAllowExpertOptions`) + `brotli_effort=11` for pixel inputs; lossless JPEG transcoding for
  JPEG inputs. Pass `--effort 10` (or lower) if faster lossless encoding is preferred on large images.
* **`--lossy` mode:** forces lossy encoding (even for JPEG inputs); quality 100 / distance 0 is rejected under `--lossy`.
* EXIF/XMP stored in `Exif`/`xml ` boxes; EXIF orientation is written to the codestream header and the stored pixels are
  not rotated. Decoding for display applies orientation; decoding for verification does not.
* Animated JXL is refused. 16-bit and alpha are kept (one straight-alpha extra channel; JXL files with premultiplied
  alpha are un-premultiplied on decode, so re-encoding them is lossless only with respect to the un-premultiplied
  values; other extra channels are refused).

## WebP (libwebp / cwebp)
* **Default mode (matches `cwebp` + max method):** lossy, quality 75 (override `--quality`), method 6 (`--method`),
  `alpha_quality = 100` (`--alpha-quality`), optional `--sharp-yuv` (`cwebp -sharp_yuv`).
  16-bit sources are reduced to 8-bit with a note.
* **`--lossless` mode:** `lossless=1`, `quality=100` (maximum lossless compression effort, equivalent to `cwebp -z 9`),
  method 6, `exact=1` (RGB under transparent pixels kept). 16-bit sources and lossy WebP→WebP are refused.
* **`--format original` on lossless WebP (VP8L):** stays lossless by default unless `--lossy` or `--quality` is passed.
* ICC/EXIF/XMP written through WebPMux; `--strip-metadata` removes EXIF/XMP (ICC kept), `--strip-icc` removes ICC.
* Animated WebP is refused. Dimension limit 16383×16383 enforced.

## JPEG (jpegoptim core + MozJPEG)
* **JPEG → JPEG (`jpegoptim` core on MozJPEG):**
  * **Default / `--lossless`:** lossless Huffman optimization, DCT coefficients and quantization tables untouched.
    `--jpeg-mode` defaults to `keep` (preserves input baseline/progressive), supports `progressive`, `baseline`, and
    `auto` (tests both baseline and progressive and keeps the smaller lossless output). JFIF version/density preserved
    (local patch L-3). Marker policy: COM/EXIF/IPTC/XMP stripped only with `--strip-metadata`; ICC only with `--strip-icc`.
  * **With `--quality N` or `--lossy`:** `jpegoptim --max` style recompression (default quality 75 when `--lossy` is
    passed without `--quality`). Runs lossless optimization as well and keeps the lossless output whenever it is
    smaller than or equal to the lossy recompression, avoiding pointless generation loss.
  * Files with data after EOI (e.g. multi-picture) are refused rather than rewritten. Truncated/damaged files are refused.
* **Non-JPEG → JPEG (MozJPEG `cjpeg` max-compression profile):**
  * **Default / `--lossy`:** quality 75 default (`--quality`), progressive by default (override `--jpeg-mode baseline`),
    trellis quantization and optimized scan scripts. Alpha is composited on white (reported), 16-bit reduced to 8-bit
    (reported), CMYK sources refused.
  * **`--lossless`:** refused because JPEG is an inherently lossy codec.

## PNG (OxiPNG + Zopfli)
* Always lossless, tuned for maximum compression: preset 6 (`Options::max_compression`) + Zopfli
  (`--zopfli-iterations`, `--zopfli-no-improvement`; `--no-zopfli` → libdeflate level 12).
* Upstream default `optimize_alpha=false` (hidden RGB under alpha 0 untouched) and `scale_16=false` (only *exact*
  bit-depth reductions). Pass `--optimize-alpha` (`oxipng --alpha`) for additional compression on transparent PNGs
  (visually lossless; disallowed under `--lossless`).
* Metadata kept unless `--strip-metadata`/`--strip-icc`. APNG animation is preserved in PNG→PNG optimization; an APNG
  cannot be a *conversion* source.

## BMP / TIFF / GIF
Decoded with the `image` crate (single-frame only). They have no "optimize in place" path: choose `--format png|webp|jpeg|jxl`.

## Verification matrix

| Route | Check |
|---|---|
| PNG→PNG / any→PNG (default & `--lossless`) | RGBA16 equality (`pixel-identical`) + semantic metadata equality (PNG→PNG) |
| PNG→PNG / any→PNG (`--optimize-alpha`) | Alpha channel exact + RGB exact wherever alpha > 0 (`visible-pixels-identical`) |
| JPEG→JPEG lossless (incl. `jpegoptim -m` lossless fallback) | All DCT coefficients, tables, dimensions, saved markers (`dct-coefficients-identical`) |
| JPEG→JXL lossless transcode | Reconstruct JPEG from JXL, byte-equal (`jpeg-bitstream-identical`) |
| Pixel lossless → JXL / WebP | RGBA16 equality (`pixel-identical`) |
| Any lossy route | Decodes cleanly, same dimensions (`decodable-lossy-not-compared`) |
| Kept original | `byte-identical-copy` |

Limits: libjxl/libwebp/libjpeg are used for both encoding and verification decoding, so a systematic bug in a library
would not be caught by the verifier alone.
