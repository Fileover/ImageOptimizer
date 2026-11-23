# Building

## GitHub Actions CI (recommended)

The repository includes [`.github/workflows/ci.yml`](.github/workflows/ci.yml), which builds and verifies the project
automatically on every push, pull request, manual trigger (`workflow_dispatch`), and version tag (`v*`):

* **`windows-x64` (`windows-latest`):** installs NASM and Ninja, fetches/caches the pinned upstream sources via
  `scripts/bootstrap.ps1`, runs the full test suite and builds `dist/ImageOptimizer.exe` statically (`/MT` +
  `+crt-static`) via `scripts/build.ps1`, verifies PE x64 and `dumpbin /dependents` via `scripts/verify-build.ps1`,
  and uploads `dist/` as the `ImageOptimizer-windows-x64` workflow artifact.
* **`linux-x64` (`ubuntu-latest`):** builds native dependencies and runs `cargo test --locked`.
* **`release` (on `v*` tags):** packages `ImageOptimizer.exe`, `ImageOptimizer.exe.sha256`, `LICENSE`, and
  `THIRD-PARTY-NOTICES.md` and publishes a GitHub Release automatically.

## Local Windows x64 build

Requirements: Windows 10/11 x64, PowerShell 7, Git, CMake ≥ 3.16, Visual Studio 2022 Build Tools (MSVC v143 +
Windows SDK), rustup, optionally NASM (MozJPEG SIMD) and Ninja.

```powershell
pwsh scripts/bootstrap.ps1 -InstallTools   # tools via winget, Rust toolchain + target, pinned native sources
pwsh scripts/build.ps1                     # tests + release build -> dist/ImageOptimizer.exe (+ sha256, licenses)
pwsh scripts/verify-build.ps1              # PE x64, dumpbin /dependents, smoke test
```

* `bootstrap.ps1` is the only step needing network; it clones the tags/commits of `native/dependencies.json`
  (libjxl with its brotli/highway/skcms submodules) and refuses mismatching commits.
* `build-native.ps1` builds only the static C/C++ libraries (cached by Cargo); `build.ps1` runs everything.
* The build is offline and reproducible: `rust-toolchain.toml` pins Rust 1.97.0, `Cargo.lock` pins crates (`--locked`).
* Static CRT: libraries use `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` (`/MT`), MozJPEG uses `WITH_CRT_DLL=OFF`, and
  Rust uses `-C target-feature=+crt-static` (`.cargo/config.toml`).
* `update-dependencies.ps1` reports newer upstream tags (`-Apply` edits `dependencies.json`; then re-run bootstrap and
  all tests).

## Linux (development / CI tests)

`cargo test --locked` works with GCC/CMake/Ninja/NASM after cloning the pinned sources into `native/` as listed in
`native/dependencies.json`.

## Troubleshooting

* `static library 'xxx' was not produced by the native build` — `build.rs` locates the CMake outputs by file stem
  (`jpeg-static.lib`, `libwebp.lib`, `jxl.lib`, `brotlienc.lib`, …). If a newer upstream renames one, adjust
  `link_named` in `build.rs`.
* `missing upstream sources` — run `scripts/bootstrap.ps1`.
* Slow JPEG coding — NASM was not found (MozJPEG built without SIMD; output identical).
