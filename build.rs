//! Build script: builds the pinned upstream codec sources as *static* libraries and compiles
//! the C shims that wrap them.
//!
//! Sources live in `native/` (see `native/dependencies.json` for the pinned tags/commits):
//!   native/libwebp    webmproject/libwebp   (BSD-3-Clause)
//!   native/libjxl     libjxl/libjxl         (BSD-3-Clause, + bundled brotli/highway/skcms)
//!   native/libjpeg    mozilla/mozjpeg       (IJG / BSD-3 / zlib)
//!   native/jpegoptim  tjko/jpegoptim        (GPL-3.0-or-later) - only jpegmarker.c is compiled;
//!                                            the optimization core is adapted in native/shims.
//!
//! Nothing here downloads anything: run scripts/bootstrap.ps1 (or the git commands in
//! BUILDING.md) first. No external codec DLL/EXE is produced or required.

use std::env;
use std::fs;
use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let native = manifest.join("native");
    let target_env = env::var("CARGO_CFG_TARGET_ENV").unwrap_or_default();
    let target_os = env::var("CARGO_CFG_TARGET_OS").unwrap_or_default();
    let is_msvc = target_env == "msvc";

    for sub in ["libwebp", "libjxl", "libjpeg", "jpegoptim"] {
        let dir = native.join(sub);
        if !dir.join("CMakeLists.txt").exists() && sub != "jpegoptim" {
            panic!(
                "missing upstream sources: {} (run scripts/bootstrap.ps1 to fetch the pinned versions)",
                dir.display()
            );
        }
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=native/shims");
    println!("cargo:rerun-if-changed=native/dependencies.json");

    // ---------------------------------------------------------------- MozJPEG
    let moz = cmake_static("mozjpeg", &native.join("libjpeg"), is_msvc, |c| {
        c.define("ENABLE_SHARED", "OFF")
            .define("ENABLE_STATIC", "ON")
            .define("WITH_TURBOJPEG", "OFF")
            .define("PNG_SUPPORTED", "OFF")
            .define("WITH_JAVA", "OFF")
            // Arithmetic *decoding* lets the optimizer read such files; we never write them.
            .define("WITH_ARITH_DEC", "ON")
            .define("WITH_ARITH_ENC", "OFF")
            // SIMD is used when NASM is available, otherwise falls back to portable C.
            .define("REQUIRE_SIMD", "OFF")
            // Static CRT (/MT) on MSVC so no VC++ redistributable is required.
            .define("WITH_CRT_DLL", "OFF");
    });

    // ---------------------------------------------------------------- libwebp
    let webp = cmake_static("libwebp", &native.join("libwebp"), is_msvc, |c| {
        c.define("WEBP_BUILD_ANIM_UTILS", "OFF")
            .define("WEBP_BUILD_CWEBP", "OFF")
            .define("WEBP_BUILD_DWEBP", "OFF")
            .define("WEBP_BUILD_GIF2WEBP", "OFF")
            .define("WEBP_BUILD_IMG2WEBP", "OFF")
            .define("WEBP_BUILD_VWEBP", "OFF")
            .define("WEBP_BUILD_WEBPINFO", "OFF")
            .define("WEBP_BUILD_WEBPMUX", "OFF")
            .define("WEBP_BUILD_EXTRAS", "OFF")
            .define("WEBP_BUILD_LIBWEBPMUX", "ON")
            .define("WEBP_USE_THREAD", "ON");
    });

    // ----------------------------------------------------------------- libjxl
    let jxl = cmake_static("libjxl", &native.join("libjxl"), is_msvc, |c| {
        c.define("BUILD_TESTING", "OFF")
            .define("JPEGXL_STATIC", "ON")
            .define("JPEGXL_ENABLE_TOOLS", "OFF")
            .define("JPEGXL_ENABLE_DEVTOOLS", "OFF")
            .define("JPEGXL_ENABLE_DOXYGEN", "OFF")
            .define("JPEGXL_ENABLE_MANPAGES", "OFF")
            .define("JPEGXL_ENABLE_BENCHMARK", "OFF")
            .define("JPEGXL_ENABLE_EXAMPLES", "OFF")
            .define("JPEGXL_ENABLE_JNI", "OFF")
            .define("JPEGXL_ENABLE_SJPEG", "OFF")
            .define("JPEGXL_ENABLE_OPENEXR", "OFF")
            .define("JPEGXL_ENABLE_JPEGLI", "OFF")
            .define("JPEGXL_ENABLE_VIEWERS", "OFF")
            .define("JPEGXL_ENABLE_PLUGINS", "OFF")
            .define("JPEGXL_ENABLE_TRANSCODE_JPEG", "ON")
            .define("JPEGXL_ENABLE_BOXES", "ON")
            .define("JPEGXL_ENABLE_SKCMS", "ON")
            .define("JPEGXL_BUNDLE_SKCMS", "ON")
            .define("JPEGXL_BUNDLE_LIBPNG", "OFF")
            .define("JPEGXL_FORCE_SYSTEM_BROTLI", "OFF")
            .define("JPEGXL_FORCE_SYSTEM_HWY", "OFF")
            .define("JPEGXL_WARNINGS_AS_ERRORS", "OFF");
    });

    // ------------------------------------------------------------------ shims
    let mut cc = cc::Build::new();
    cc.warnings(true)
        .include(native.join("shims"))
        .include(native.join("jpegoptim")) // jpegmarker.h
        .include(native.join("libjpeg")) // jpeglib.h, jerror.h
        .include(moz.build_dir.clone()) // generated jconfig.h
        .include(webp.out_dir.join("include"))
        .include(jxl.out_dir.join("include"))
        .include(native.join("libjxl/lib/include"))
        .include(jxl.build_dir.join("lib/include")) // generated jxl/version.h, jxl_export.h
        .file(native.join("shims/shim_common.c"))
        .file(native.join("shims/shim_webp.c"))
        .file(native.join("shims/shim_jxl.c"))
        .file(native.join("shims/shim_jpeg.c"))
        .file(native.join("shims/shim_jpegoptim.c"))
        .file(native.join("jpegoptim/jpegmarker.c")); // upstream file, unmodified
    cc.define("JXL_STATIC_DEFINE", None);
    cc.define("JXL_THREADS_STATIC_DEFINE", None);
    cc.define("JXL_CMS_STATIC_DEFINE", None);
    if is_msvc {
        cc.static_crt(true);
        cc.define("_CRT_SECURE_NO_WARNINGS", None);
    }
    cc.compile("io_shims");

    // Link order matters for GNU ld: dependents first, then their dependencies.
    link_named(&jxl.libs, &["jxl_threads", "jxl", "jxl_cms", "skcms"], is_msvc);
    link_named(&jxl.libs, &["brotlienc", "brotlidec", "brotlicommon", "hwy"], is_msvc);
    link_named(&webp.libs, &["webpmux", "webpdemux", "webp", "sharpyuv"], is_msvc);
    link_named(&moz.libs, &["jpeg"], is_msvc);

    match target_os.as_str() {
        "linux" | "android" => println!("cargo:rustc-link-lib=stdc++"),
        "macos" | "ios" => println!("cargo:rustc-link-lib=c++"),
        _ => {} // MSVC: the C++ runtime is linked statically through /MT
    }
}

struct Built {
    out_dir: PathBuf,
    build_dir: PathBuf,
    libs: Vec<PathBuf>,
}

fn cmake_static(
    name: &str,
    src: &Path,
    is_msvc: bool,
    configure: impl FnOnce(&mut cmake::Config),
) -> Built {
    // Every library gets its own build/install tree below OUT_DIR (the install prefix is set
    // by the cmake crate to that tree; never override it - it must stay inside OUT_DIR).
    let dest = PathBuf::from(env::var("OUT_DIR").unwrap()).join(name);
    let mut cfg = cmake::Config::new(src);
    cfg.out_dir(&dest).profile("Release")
        .define("BUILD_SHARED_LIBS", "OFF")
        .define("CMAKE_POSITION_INDEPENDENT_CODE", "ON")
        // Build every dependency with the static CRT on MSVC (/MT) so that the final
        // executable does not need the Visual C++ Redistributable.
        .define("CMAKE_POLICY_DEFAULT_CMP0091", "NEW")
        // CMake 4.x removed compatibility with cmake_minimum_required < 3.5 (used by MozJPEG 4.1.5).
        .define("CMAKE_POLICY_VERSION_MINIMUM", "3.5");
    if is_msvc {
        cfg.define("CMAKE_MSVC_RUNTIME_LIBRARY", "MultiThreaded");
        cfg.static_crt(true);
    }
    configure(&mut cfg);
    let out_dir = cfg.build();
    let build_dir = out_dir.join("build");
    let mut libs = Vec::new();
    collect_libs(&out_dir, &mut libs);
    // Prefer installed libraries under `lib/` or `lib64/` over intermediate files in `build/`.
    libs.sort_by_key(|p| {
        let in_build = p.starts_with(&build_dir);
        (in_build, p.clone())
    });
    Built { out_dir, build_dir, libs }
}

fn collect_libs(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_libs(&p, out);
        } else if matches!(p.extension().and_then(|x| x.to_str()), Some("a") | Some("lib")) {
            out.push(p);
        }
    }
}

/// Maps a file name such as `libbrotlienc-static.a` / `brotlienc-static.lib` to `brotlienc`.
fn lib_stem(p: &Path) -> String {
    let stem = p.file_stem().unwrap().to_string_lossy().to_string();
    let stem = stem.strip_prefix("lib").unwrap_or(&stem).to_string();
    stem.strip_suffix("-static").unwrap_or(&stem).to_string()
}

fn link_named(libs: &[PathBuf], wanted: &[&str], is_msvc: bool) {
    for name in wanted {
        // On MSVC mozjpeg's static library is `jpeg-static.lib`; elsewhere `libjpeg.a`.
        let found = libs.iter().find(|p| lib_stem(p) == *name);
        match found {
            Some(p) => {
                println!("cargo:rustc-link-search=native={}", p.parent().unwrap().display());
                let file = p.file_stem().unwrap().to_string_lossy().to_string();
                let linkname = if is_msvc || !file.starts_with("lib") {
                    file.clone()
                } else {
                    file.trim_start_matches("lib").to_string()
                };
                println!("cargo:rustc-link-lib=static={linkname}");
            }
            None => {
                // skcms may be merged into jxl_cms depending on the libjxl version.
                if *name != "skcms" {
                    let available: Vec<String> = libs.iter().map(|p| p.display().to_string()).collect();
                    panic!(
                        "static library '{name}' was not produced by the native build (found: {:?})",
                        available
                    );
                }
            }
        }
    }
}
