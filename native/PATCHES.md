# Local patches and adaptations

## Upstream sources

No upstream source file in `native/libwebp`, `native/libjxl` (incl. submodules), `native/libjpeg`
(MozJPEG) or `native/jpegoptim` is modified. `git -C native/<dir> status` must be clean after
`scripts/bootstrap.ps1`; `scripts/verify-build.ps1` checks the pinned commits.

`native/jpegoptim/jpegmarker.c` and `jpegmarker.h` are compiled **unmodified**.

## jpegoptim adaptation (L-1 … L-6)

`native/shims/shim_jpegoptim.c` is a derivative work of `jpegoptim.c` (tag v1.5.6, GPL-3.0-or-later,
Copyright (C) 1996-2022 Timo Kokkonen). This is the only reason the combined executable is
distributed under GPL-3.0-or-later (see `LICENSES.md`).

| ID  | Change | Reason |
|-----|--------|--------|
| L-1 | File/stdin/stdout handling, option parsing, logging, target-size search and multi-file loop removed; the core works on memory buffers (`jpeg_mem_src`/`jpeg_mem_dest`). | Single executable, no process spawning, atomic writes handled by the application. |
| L-2 | `setjmp`/`longjmp` error handler returning error codes. | Upstream calls `exit()`. |
| L-3 | JFIF version and density (units, X, Y) copied explicitly after `jpeg_copy_critical_parameters()`. | That call resets them; losing the DPI of a lossless optimization is a bug. |
| L-4 | Adobe APP14 marker not written twice (CMYK/YCCK). | libjpeg already emits it. |
| L-5 | Data after EOI is reported (`trailing_bytes`) and the application refuses to rewrite such files. | Appended/multi-picture data must never be dropped silently. |
| L-6 | `shim_jpeg_coefficients_equal()` added. | Post-optimization verification at DCT-coefficient level. |
| L-7 | libjpeg warnings are promoted to errors (`emit_message` handler) in `shim_jpegoptim.c` and in the decode path of `shim_jpeg.c`. | libjpeg otherwise silently "repairs" truncated/damaged files; corrupt inputs must be rejected rather than rewritten into a different, valid-looking file. |

## Build-system notes

* `build.rs` drives the upstream CMake projects with the `cmake` crate; no `CMakeLists.txt` is edited.
* All libraries are built with `CMAKE_MSVC_RUNTIME_LIBRARY=MultiThreaded` (`/MT`) and the Rust target uses
  `-C target-feature=+crt-static` (`.cargo/config.toml`), so no VC++ redistributable is needed.
