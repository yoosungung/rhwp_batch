use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(name = "rhwp-batch", version, about = "HWP/HWPX batch conversion and template filling")]
pub struct Cli {
    #[command(subcommand)]
    pub command: Command,

    /// Log format (text | json)
    #[arg(long, global = true, default_value = "text")]
    pub log_format: String,

    /// Log level (error | warn | info | debug | trace)
    #[arg(long, global = true, default_value = "info")]
    pub log_level: String,
}

#[derive(Subcommand)]
pub enum Command {
    /// Convert HWP/HWPX to JSON
    ToJson(ToJsonArgs),
    /// Fill a template HWP with JSON data
    Fill(FillArgs),
}

#[derive(clap::Args)]
pub struct ToJsonArgs {
    /// Input HWP/HWPX file
    #[arg(short, long, conflicts_with = "input_dir")]
    pub input: Option<std::path::PathBuf>,

    /// Input directory (process all HWP/HWPX files)
    #[arg(long, conflicts_with = "input")]
    pub input_dir: Option<std::path::PathBuf>,

    /// Output JSON file (single file mode)
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,

    /// Output directory (directory mode)
    #[arg(long)]
    pub output_dir: Option<std::path::PathBuf>,

    /// Image handling mode
    #[arg(long, default_value = "extract")]
    pub image_mode: ImageModeArg,

    /// Directory to write extracted images (default: <output>.assets/)
    #[arg(long)]
    pub image_dir: Option<std::path::PathBuf>,

    /// Pretty-print JSON output
    #[arg(long)]
    pub pretty: bool,

    /// Heading style map overrides (e.g. "MyStyle=2")
    #[arg(long, value_name = "NAME=LEVEL")]
    pub heading_style_map: Vec<String>,

    /// Overwrite existing output files
    #[arg(long)]
    pub overwrite: bool,
}

#[derive(clap::Args)]
pub struct FillArgs {
    /// Template HWP file path
    #[arg(long)]
    pub template: std::path::PathBuf,

    /// Input JSON data file (single mode)
    #[arg(long, conflicts_with_all = ["data_dir", "manifest"])]
    pub data: Option<std::path::PathBuf>,

    /// Input data directory (batch mode)
    #[arg(long, conflicts_with_all = ["data", "manifest"])]
    pub data_dir: Option<std::path::PathBuf>,

    /// Batch manifest JSON file
    #[arg(long, conflicts_with_all = ["data", "data_dir"])]
    pub manifest: Option<std::path::PathBuf>,

    /// Output HWP file (single mode)
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,

    /// Output directory (batch mode)
    #[arg(long)]
    pub output_dir: Option<std::path::PathBuf>,

    /// Batch report output path
    #[arg(long)]
    pub report: Option<std::path::PathBuf>,

    /// Behavior on missing template key (error | empty | keep)
    #[arg(long, default_value = "error")]
    pub on_missing_key: OnMissingKey,

    /// Behavior on per-item error in batch (stop | continue)
    #[arg(long, default_value = "stop")]
    pub on_error: OnError,

    /// Number of worker threads (default: CPU count)
    #[arg(long)]
    pub threads: Option<usize>,

    /// Overwrite existing output files
    #[arg(long)]
    pub overwrite: bool,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum ImageModeArg {
    Extract,
    Inline,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OnMissingKey {
    Error,
    Empty,
    Keep,
}

#[derive(Debug, Clone, Copy, ValueEnum, PartialEq)]
pub enum OnError {
    Stop,
    Continue,
}
