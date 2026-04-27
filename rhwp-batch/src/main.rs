use clap::Parser;
use rhwp_batch::batch::{BatchConfig, items_from_data_dir, items_from_manifest, run_batch};
use rhwp_batch::cli::{Cli, Command};
use rhwp_batch::error::BatchError;
use rhwp_batch::service::{ToJsonConfig, fill_single, to_json_dir, to_json_single};
use tracing_subscriber::fmt;

fn main() {
    let cli = Cli::parse();
    init_logging(&cli.log_format, &cli.log_level);

    let exit_code = match cli.command {
        Command::ToJson(args) => run_to_json(args),
        Command::Fill(args) => run_fill(args),
    };
    std::process::exit(exit_code);
}

fn run_to_json(args: rhwp_batch::cli::ToJsonArgs) -> i32 {
    let cfg = ToJsonConfig {
        image_mode: args.image_mode,
        image_dir: args.image_dir.clone(),
        pretty: args.pretty,
        heading_style_map: args.heading_style_map.clone(),
        overwrite: args.overwrite,
    };

    if let Some(input_dir) = &args.input_dir {
        let Some(output_dir) = &args.output_dir else {
            tracing::error!("--output-dir is required when using --input-dir");
            return 2;
        };
        match to_json_dir(input_dir, output_dir, &cfg) {
            Ok(code) => code,
            Err(e) => {
                tracing::error!("{}", e);
                1
            }
        }
    } else if let Some(input) = &args.input {
        let output = match &args.output {
            Some(p) => p.clone(),
            None => {
                let stem = input.file_stem().and_then(|s| s.to_str()).unwrap_or("out");
                let parent = input.parent().unwrap_or(std::path::Path::new("."));
                parent.join(format!("{}.json", stem))
            }
        };
        match to_json_single(input, &output, &cfg) {
            Ok(_) => 0,
            Err(BatchError::OutputExists(p)) => {
                tracing::error!("output exists (use --overwrite): {}", p);
                3
            }
            Err(e) => {
                tracing::error!("{}", e);
                1
            }
        }
    } else {
        tracing::error!("either --input or --input-dir is required");
        2
    }
}

fn run_fill(args: rhwp_batch::cli::FillArgs) -> i32 {
    let threads = args.threads.unwrap_or_else(|| {
        std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(1)
    });

    // batch mode: --data-dir or --manifest
    if args.data_dir.is_some() || args.manifest.is_some() {
        let Some(output_dir) = &args.output_dir else {
            tracing::error!("--output-dir is required in batch mode");
            return 2;
        };

        let items = if let Some(data_dir) = &args.data_dir {
            match items_from_data_dir(data_dir, output_dir) {
                Ok(i) => i,
                Err(e) => { tracing::error!("{}", e); return 1; }
            }
        } else {
            let manifest = args.manifest.as_ref().unwrap();
            match items_from_manifest(manifest) {
                Ok(i) => i,
                Err(e) => { tracing::error!("{}", e); return 1; }
            }
        };

        let cfg = BatchConfig {
            on_missing_key: args.on_missing_key,
            on_error_continue: args.on_error == rhwp_batch::cli::OnError::Continue,
            threads,
            overwrite: args.overwrite,
        };

        let (exit_code, report) = run_batch(&args.template, &items, &cfg);

        // Write report if requested
        if let Some(report_path) = &args.report {
            match serde_json::to_string_pretty(&report) {
                Ok(json) => {
                    if let Err(e) = std::fs::write(report_path, json) {
                        tracing::error!("failed to write report: {}", e);
                    }
                }
                Err(e) => tracing::error!("failed to serialize report: {}", e),
            }
        }

        tracing::info!(
            total = report.total,
            succeeded = report.succeeded,
            failed = report.failed,
            "batch complete"
        );
        return exit_code;
    }

    // single mode
    let Some(data_path) = &args.data else {
        tracing::error!("one of --data, --data-dir, --manifest is required");
        return 2;
    };
    let output = match &args.output {
        Some(p) => p.clone(),
        None => {
            tracing::error!("--output is required in single fill mode");
            return 2;
        }
    };

    match fill_single(&args.template, data_path, &output, args.on_missing_key, args.overwrite) {
        Ok(_) => 0,
        Err(BatchError::OutputExists(p)) => {
            tracing::error!("output exists (use --overwrite): {}", p);
            3
        }
        Err(BatchError::MissingKey(k)) => {
            tracing::error!("missing template key: {}", k);
            1
        }
        Err(e) => {
            tracing::error!("{}", e);
            1
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
