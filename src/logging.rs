use tracing_subscriber::EnvFilter;

/// Initializes `tracing` (stderr). `RUST_LOG` overrides the verbosity flags.
pub fn init(verbose: u8, quiet: bool) {
    let level = if quiet {
        "error"
    } else {
        match verbose {
            0 => "warn",
            1 => "info",
            _ => "debug",
        }
    };
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new(level));
    let _ = tracing_subscriber::fmt().with_env_filter(filter).with_target(false).with_writer(std::io::stderr).try_init();
}
