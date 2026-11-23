//! Coordinated scheduling: files in flight (`jobs`) x codec threads per file (`threads`) must
//! not exceed the number of logical processors, otherwise 16 parallel files with 16 encoder
//! threads each would oversubscribe the machine.

use sysinfo::System;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Plan {
    pub jobs: usize,
    pub threads: usize,
    pub cpus: usize,
    pub oversubscribed: bool,
}

/// Rough working-set estimate per concurrently processed image (decoded raster + encoder
/// state); used only to cap the default `jobs`.
const MEM_PER_JOB_MB: u64 = 768;

pub fn logical_cpus() -> usize {
    std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1)
}

pub fn available_memory_mb() -> u64 {
    let mut sys = System::new();
    sys.refresh_memory();
    let avail = sys.available_memory();
    if avail == 0 {
        4096
    } else {
        avail / (1024 * 1024)
    }
}

pub fn plan(files: usize, jobs: Option<usize>, threads: Option<usize>, cpus: usize, mem_mb: u64) -> Plan {
    let cpus = cpus.max(1);
    let files = files.max(1);
    let mem_cap = ((mem_mb / MEM_PER_JOB_MB).max(1)) as usize;
    let (jobs, threads) = match (jobs, threads) {
        (Some(j), Some(t)) => (j, t),
        (Some(j), None) => (j, (cpus / j.max(1)).max(1)),
        (None, Some(t)) => ((cpus / t.max(1)).max(1).min(files), t),
        (None, None) => {
            let j = cpus.min(mem_cap).min(files).max(1);
            (j, (cpus / j).max(1))
        }
    };
    Plan { jobs, threads, cpus, oversubscribed: jobs * threads > cpus }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn single_file_gets_all_threads() {
        let p = plan(1, None, None, 16, 32_000);
        assert_eq!((p.jobs, p.threads), (1, 16));
    }

    #[test]
    fn many_files_do_not_oversubscribe() {
        let p = plan(500, None, None, 16, 32_000);
        assert!(p.jobs * p.threads <= 16);
        assert!(!p.oversubscribed);
    }

    #[test]
    fn memory_caps_jobs() {
        let p = plan(500, None, None, 16, 1_600);
        assert_eq!(p.jobs, 2);
    }

    #[test]
    fn explicit_oversubscription_is_flagged() {
        assert!(plan(10, Some(8), Some(8), 8, 32_000).oversubscribed);
    }
}
