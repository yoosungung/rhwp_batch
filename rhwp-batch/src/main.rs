use clap::Parser;
use rhwp_batch::cli::{Cli, Command};
use tracing_subscriber::fmt;

fn main() {
    let cli = Cli::parse();
    init_logging(&cli.log_format, &cli.log_level);

    match cli.command {
        Command::ToJson(_args) => {
            tracing::error!("to-json not yet implemented (M2)");
            std::process::exit(1);
        }
        Command::Fill(_args) => {
            tracing::error!("fill not yet implemented (M3)");
            std::process::exit(1);
        }
    }
}

fn init_logging(format: &str, level: &str) {
    let level_filter = level
        .parse::<tracing::Level>()
        .unwrap_or(tracing::Level::INFO);

    if format == "json" {
        tracing_subscriber::fmt()
            .json()
            .with_max_level(level_filter)
            .init();
    } else {
        fmt().with_max_level(level_filter).init();
    }
}
