# Licensing audit

**Result: the distributed executable is licensed GPL-3.0-or-later.**

| Component | License | Effect |
|---|---|---|
| jpegoptim 1.5.6 | GPL-3.0-or-later | Its optimizer logic is adapted into `native/shims/shim_jpegoptim.c` and `jpegmarker.c` is compiled in. The combined work is therefore a derivative and must be distributed under GPL-3.0-or-later. |
| libjxl, libwebp, MozJPEG/libjpeg-turbo/IJG, brotli, skcms, highway | BSD-3 / MIT / Apache-2.0 / IJG / zlib | GPL-compatible; notices must be kept (done in `THIRD-PARTY-NOTICES.md`). Apache-2.0 is compatible with GPL-3.0. |
| oxipng, zopfli, libdeflater, other Rust crates | MIT / Apache-2.0 / Unlicense | GPL-3.0 compatible. |

Obligations when distributing the binary: ship `LICENSE` (GPL-3.0) and `THIRD-PARTY-NOTICES.md` (both are staged in `dist/`),
offer the **corresponding source** (this repository incl. `native/shims`, `Cargo.lock`, `native/dependencies.json`, scripts) to every
recipient, and keep the copyright headers (e.g. in `shim_jpegoptim.c`).

Alternative if GPL is unacceptable: replace the jpegoptim core by a clean-room lossless optimizer built only on MozJPEG
(`jpeg_read_coefficients` + `jpeg_write_coefficients` + `optimize_coding`, plus own marker handling). Everything else in the
project is permissively licensed, and the project could then be released under MIT/Apache-2.0. This is not a legal opinion.
