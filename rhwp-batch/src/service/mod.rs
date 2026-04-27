use std::collections::HashMap;
use std::path::{Path, PathBuf};

use rhwp::parser::{FileFormat, detect_format, parse_document};
use tracing::instrument;
use walkdir::WalkDir;

use crate::adapter::{ConvertOptions, ImageMode, convert, sha256_file};
use crate::cli::{ImageModeArg, OnMissingKey};
use crate::error::BatchError;

pub type ExitCode = i32;

// ── to-json ───────────────────────────────────────────────────────────────────

pub struct ToJsonConfig {
    pub image_mode: ImageModeArg,
    pub image_dir: Option<PathBuf>,
    pub pretty: bool,
    pub heading_style_map: Vec<String>,
    pub overwrite: bool,
}

#[instrument(skip_all, fields(input = %input.display()))]
pub fn to_json_single(
    input: &Path,
    output: &Path,
    cfg: &ToJsonConfig,
) -> Result<(), BatchError> {
    if output.exists() && !cfg.overwrite {
        return Err(BatchError::OutputExists(output.display().to_string()));
    }

    let data = std::fs::read(input)?;
    let sha256 = sha256_file(&data);
    let format_str = format_name(&data);

    tracing::info!(file = %input.display(), sha256 = %sha256, "parsing");
    let doc = parse_document(&data)
        .map_err(|e| BatchError::Parse(format!("{:?}", e)))?;

    let image_dir = cfg.image_dir.clone().unwrap_or_else(|| resolve_image_dir(output));
    let heading_style_map = parse_heading_style_map(&cfg.heading_style_map);

    let opts = ConvertOptions {
        source_filename: input
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_string(),
        source_format: format_str,
        source_sha256: sha256,
        image_mode: match cfg.image_mode {
            ImageModeArg::Inline => ImageMode::Inline,
            ImageModeArg::Extract => ImageMode::Extract,
        },
        image_dir: if cfg.image_mode == ImageModeArg::Extract {
            Some(image_dir)
        } else {
            None
        },
        heading_style_map,
    };

    let doc_json = convert(&doc, &opts);

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }

    let json_bytes = if cfg.pretty {
        serde_json::to_vec_pretty(&doc_json)?
    } else {
        serde_json::to_vec(&doc_json)?
    };
    std::fs::write(output, &json_bytes)?;
    tracing::info!(output = %output.display(), blocks = doc_json.blocks.len(), "written");
    Ok(())
}

pub fn to_json_dir(
    input_dir: &Path,
    output_dir: &Path,
    cfg: &ToJsonConfig,
) -> Result<ExitCode, BatchError> {
    std::fs::create_dir_all(output_dir)?;
    let mut any_error = false;

    for entry in WalkDir::new(input_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext != "hwp" && ext != "hwpx" {
            continue;
        }

        let rel = path.strip_prefix(input_dir).unwrap_or(path);
        let out = output_dir.join(rel.with_extension("json"));

        // per-file image_dir: <output_dir>/<stem>.assets/  (overridden if cfg.image_dir set)
        let file_image_dir = cfg.image_dir.clone().unwrap_or_else(|| {
            output_dir.join(rel.with_extension("assets"))
        });

        let file_cfg = ToJsonConfig {
            image_mode: cfg.image_mode,
            image_dir: Some(file_image_dir),
            pretty: cfg.pretty,
            heading_style_map: cfg.heading_style_map.clone(),
            overwrite: cfg.overwrite,
        };

        match to_json_single(path, &out, &file_cfg) {
            Ok(_) => {}
            Err(BatchError::OutputExists(p)) => {
                tracing::warn!(path = %p, "skipping existing output");
            }
            Err(e) => {
                tracing::error!(file = %path.display(), error = %e, "conversion failed");
                any_error = true;
            }
        }
    }

    Ok(if any_error { 1 } else { 0 })
}

// ── fill ──────────────────────────────────────────────────────────────────────

pub fn fill_single(
    template: &Path,
    data_path: &Path,
    output: &Path,
    on_missing: OnMissingKey,
    overwrite: bool,
) -> Result<(), BatchError> {
    if output.exists() && !overwrite {
        return Err(BatchError::OutputExists(output.display().to_string()));
    }

    let template_bytes = std::fs::read(template)
        .map_err(|e| BatchError::TemplateNotFound(format!("{}: {}", template.display(), e)))?;
    let doc = parse_document(&template_bytes)
        .map_err(|e| BatchError::Parse(format!("{:?}", e)))?;

    let json_str = std::fs::read_to_string(data_path)?;
    let data: serde_json::Value = serde_json::from_str(&json_str)?;

    let filled = crate::template::fill_document(doc, &data, on_missing)?;

    let out_bytes = rhwp::serializer::serialize_hwp(&filled)
        .map_err(|e| BatchError::Serialize(format!("{:?}", e)))?;

    if let Some(parent) = output.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(output, out_bytes)?;
    tracing::info!(output = %output.display(), "filled");
    Ok(())
}

// ── helpers ───────────────────────────────────────────────────────────────────

fn format_name(data: &[u8]) -> String {
    match detect_format(data) {
        FileFormat::Hwpx => "hwpx".to_string(),
        _ => "hwp".to_string(),
    }
}

fn resolve_image_dir(output: &Path) -> PathBuf {
    let stem = output
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("out");
    let parent = output.parent().unwrap_or(Path::new("."));
    parent.join(format!("{}.assets", stem))
}

fn parse_heading_style_map(pairs: &[String]) -> HashMap<String, u8> {
    pairs
        .iter()
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            let level: u8 = v.trim().parse().ok()?;
            if (1..=6).contains(&level) {
                Some((k.trim().to_string(), level))
            } else {
                None
            }
        })
        .collect()
}
