# Architecture

```
src/
  main.rs / app.rs         CLI entry, discovery, scheduling, progress, reports, exit codes
  cli.rs config.rs         clap options; JSON config merge (CLI > file > defaults); mode & range validation
  processing/              pipeline.rs (per file, effective mode routing), scheduler.rs (jobs x threads x memory)
  formats/                 jpegxl.rs webp.rs jpeg.rs png.rs : upstream defaults, max optimization, --lossless, verify
  engines/                 libjxl.rs libwebp.rs mozjpeg.rs jpegoptim.rs oxipng.rs : safe wrappers; ffi.rs raw FFI
  image/                   identification (magic bytes), decoder, encoder (PNG writer), color (Raster), metadata
  reporting/               statistics, csv, json
  utils/                   filesystem (long paths, scan, output planner), temporary_files (atomic write), formatting
native/
  shims/                   C shims including the real upstream headers; flat ABI in io_shim.h
  libwebp libjxl libjpeg jpegoptim   pinned, unmodified upstream sources
build.rs                   builds the upstream CMake projects statically + the shims
```

**FFI design.** Rust never declares upstream structs. Each codec has a C shim that includes the real headers and
exposes a flat ABI (`io_shim.h`: `io_image`, `io_buffer`, parameter structs, integer error codes + message buffer).
Buffers are `malloc`ed in C and owned by RAII wrappers in Rust (`OwnedBuffer`/`OwnedImage`). Struct layout is always
computed by the C compiler. Errors from libjpeg use `setjmp/longjmp` inside the shim only.

**Compression modes.** `Mode::Default` follows each upstream tool's default behavior (`cjxl`, `cwebp`, `jpegoptim`,
MozJPEG `cjpeg`, `oxipng`) with maximum compression effort. `Mode::Lossless` (`--lossless`) enforces strict
mathematically lossless encoding/transcoding across all formats and refuses routes that require loss. `Mode::Lossy`
(`--lossy`) forces lossy encoding even on routes whose upstream default is lossless (JPEG→JPEG, JPEG→JXL). Each
`FileResult` records the actual effective mode (`lossless` or `lossy`) applied to that file.

**Per-file pipeline** (`processing/pipeline.rs`): read → identify (magic bytes) → route & resolve effective mode →
decode (when needed) → encode/optimize → verify → keep-original decision → atomic write (temp file next to the
destination, rename). Every stage is timed. Each file runs inside `catch_unwind`; a failure is reported for that file
and the run continues. Cancellation (Ctrl+C) is checked between stages; the second Ctrl+C aborts.

**Scheduling.** `--jobs` files run in a Rayon pool; each file's codec gets `--threads`. When unset, the planner picks
values with `jobs × threads ≤ logical CPUs`, and caps jobs by available memory using a fixed budget of 768 MiB per job
(`MEM_PER_JOB_MB`). The budget is not derived from image size: very large images (up to `--max-pixels`) can exceed it,
so lower `--jobs` for such batches. Explicit oversubscription is allowed but logged. OxiPNG's internal parallelism runs
inside the same pool.

**Canonical comparison.** Lossless pixel verification decodes both sides to RGBA16 (`color::to_rgba16`: 8-bit scaled by
257, gray expanded, alpha 1.0 when absent) so formats with different layouts can be compared exactly. Alpha-0 pixels
are compared including their RGB values (unless `--optimize-alpha` is enabled for PNG, which verifies alpha equality and
RGB equality for every pixel with alpha > 0).

**Output planning.** `<output>/<Format dir>/<relative dir>/<name>.<ext>`; name collisions caused by different sources
(`a.png`, `a.jpg` → `a.jxl`) get a deterministic suffix (`a-jpg.jxl`).

**Safety rules**: sources opened read-only; destination ≠ source checked; temp files cleaned on failure; no output is
written if verification fails; limits on pixel count (`--max-pixels`) and decompressed size.
