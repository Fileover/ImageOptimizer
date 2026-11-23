use clap::Parser;
use image_optimizer::{app, cli::Cli};

fn main() {
    let cli = Cli::parse();
    let code = app::run(&cli);
    std::process::exit(code);
}
