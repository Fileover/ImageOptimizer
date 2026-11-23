//! Temporary output files. A [`TempFile`] is created next to the final destination so that the
//! final rename is atomic (same volume). It is removed on drop unless persisted, so a failed or
//! cancelled conversion never leaves a partial output behind.

use crate::errors::{Error, Result};
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub struct TempFile {
    path: PathBuf,
    keep: bool,
}

impl TempFile {
    /// Writes `data` to a fresh temporary file in the directory of `dest` and flushes it to disk.
    pub fn write_next_to(dest: &Path, data: &[u8]) -> Result<Self> {
        let dir = dest.parent().unwrap_or_else(|| Path::new("."));
        let name = dest.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let n = COUNTER.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".{name}.imgopt-{}-{n}.tmp", std::process::id()));
        let guard = Self { path: tmp.clone(), keep: false };
        let mut f = File::options()
            .write(true)
            .create_new(true)
            .open(&tmp)
            .map_err(|e| Error::io(&tmp, e))?;
        f.write_all(data).map_err(|e| Error::io(&tmp, e))?;
        f.sync_all().map_err(|e| Error::io(&tmp, e))?;
        Ok(guard)
    }

    /// Atomically moves the temporary file to `dest` (replacing it if `overwrite` allows).
    pub fn persist(mut self, dest: &Path, overwrite: bool) -> Result<()> {
        if dest.exists() {
            if !overwrite {
                return Err(Error::Invalid(format!(
                    "output file already exists (use --overwrite): {}",
                    dest.display()
                )));
            }
            // A read-only destination would make the replacement fail on Windows.
            if let Ok(meta) = fs::metadata(dest) {
                let mut perm = meta.permissions();
                if perm.readonly() {
                    #[allow(clippy::permissions_set_readonly_false)]
                    perm.set_readonly(false);
                    let _ = fs::set_permissions(dest, perm);
                }
            }
        }
        fs::rename(&self.path, dest).map_err(|e| Error::io(dest, e))?;
        self.keep = true;
        Ok(())
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        if !self.keep {
            let _ = fs::remove_file(&self.path);
        }
    }
}
