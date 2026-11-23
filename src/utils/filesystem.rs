//! Filesystem helpers: long-path handling, directory scanning, output path planning.

use crate::errors::{Error, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// On Windows, paths longer than MAX_PATH need the `\\?\` prefix. The prefix is only added to
/// absolute paths and never to paths that already carry it. No-op on other platforms.
pub fn long_path(p: &Path) -> PathBuf {
    #[cfg(windows)]
    {
        use std::path::Component;
        let s = p.as_os_str().to_string_lossy();
        if s.starts_with(r"\\?\") || s.len() < 240 {
            return p.to_path_buf();
        }
        if let Ok(abs) = std::path::absolute(p) {
            if matches!(abs.components().next(), Some(Component::Prefix(_))) {
                let a = abs.to_string_lossy();
                if let Some(rest) = a.strip_prefix(r"\\") {
                    return PathBuf::from(format!(r"\\?\UNC\{rest}"));
                }
                return PathBuf::from(format!(r"\\?\{a}"));
            }
        }
    }
    p.to_path_buf()
}

#[derive(Debug, Clone)]
pub struct InputFile {
    pub path: PathBuf,
    /// Directory of the file relative to the input root (empty for a single input file).
    pub rel_dir: PathBuf,
}

pub fn scan_inputs(input: &Path, recursive: bool, exclude: &Path) -> Result<Vec<InputFile>> {
    let meta = std::fs::metadata(long_path(input)).map_err(|e| Error::io(input, e))?;
    if meta.is_file() {
        return Ok(vec![InputFile { path: input.to_path_buf(), rel_dir: PathBuf::new() }]);
    }
    let exclude = std::fs::canonicalize(exclude).unwrap_or_else(|_| exclude.to_path_buf());
    let mut walker = walkdir::WalkDir::new(input).follow_links(false);
    if !recursive {
        walker = walker.max_depth(1);
    }
    let mut files = Vec::new();
    for entry in walker.sort_by_file_name() {
        let entry = entry.map_err(|e| {
            let p = e.path().map(Path::to_path_buf).unwrap_or_else(|| input.to_path_buf());
            Error::io(p, e.into_io_error().unwrap_or_else(|| std::io::Error::other("walk error")))
        })?;
        if !entry.file_type().is_file() {
            continue;
        }
        let p = entry.path();
        if std::fs::canonicalize(p).map(|c| c.starts_with(&exclude)).unwrap_or(false) {
            continue; // never re-process our own output
        }
        let rel = p.strip_prefix(input).unwrap_or(p);
        files.push(InputFile {
            path: p.to_path_buf(),
            rel_dir: rel.parent().map(Path::to_path_buf).unwrap_or_default(),
        });
    }
    Ok(files)
}

/// Hands out unique output paths: when two inputs would map to the same output name (for
/// example `a.png` and `a.jpg` both converted to WebP) the second one gets the source
/// extension appended (`a-jpg.webp`).
#[derive(Default)]
pub struct OutputPlanner {
    used: HashSet<String>,
}

impl OutputPlanner {
    pub fn plan(&mut self, dir: &Path, src: &Path, out_ext: &str) -> PathBuf {
        let stem = src.file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
        let src_ext = src.extension().map(|s| s.to_string_lossy().to_lowercase()).unwrap_or_default();
        let mut candidate = dir.join(format!("{stem}.{out_ext}"));
        if !self.used.insert(key(&candidate)) {
            candidate = dir.join(format!("{stem}-{src_ext}.{out_ext}"));
            let mut n = 2;
            while !self.used.insert(key(&candidate)) {
                candidate = dir.join(format!("{stem}-{src_ext}-{n}.{out_ext}"));
                n += 1;
            }
        }
        candidate
    }
}

fn key(p: &Path) -> String {
    // Windows and macOS file systems are case-insensitive by default.
    p.to_string_lossy().to_lowercase()
}
