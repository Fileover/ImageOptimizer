use clap::Parser;
use image_optimizer::{app, cli::Cli};

fn main() {
    if std::env::args_os().len() <= 1 {
        print!("{}", app::intro_text());
        std::process::exit(app::EXIT_OK);
    }
    let cli = Cli::parse();
    let code = app::run(&cli);
    std::process::exit(code);
}
