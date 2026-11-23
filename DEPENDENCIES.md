# Dependencies

Authoritative list with pinned tags and commit SHAs: [`native/dependencies.json`](native/dependencies.json).

| Component | Version | License | Linked |
|---|---|---|---|
| libwebp | v1.6.0 (4fa2191) | BSD-3-Clause | static |
| libjxl | v0.13.0 (5a7da83) | BSD-3-Clause | static |
| ↳ brotli / highway / skcms | db24a98 / 457c891 / 96d9171 | MIT / Apache-2.0 / BSD-3 | static |
| MozJPEG | v4.1.5 (6c9f089) | IJG + BSD-3 + zlib | static |
| jpegoptim | v1.5.6 (d0c79a8) | **GPL-3.0-or-later** | adapted source, static |
| oxipng (crate) | 10.2.1 | MIT | static |
| zopfli / libdeflater | 0.8.4 / 1.26.1 | Apache-2.0 / Apache-2.0 + MIT | static |
| png, image, rayon, clap, csv, sysinfo, ctrlc, walkdir, serde, tracing… | see `Cargo.lock` | MIT/Apache-2.0 family | static |

Upstream sources are **unmodified**; the only local code changes are in `native/shims/` (see `native/PATCHES.md`).
Updating: `scripts/update-dependencies.ps1`, then bootstrap, `scripts/build.ps1` (runs all tests), review the changelogs of the
libraries (especially libjxl bitstream/encoder behaviour changes), and update `CHANGELOG.md`, `THIRD-PARTY-NOTICES.md`.
