//! 배치 러너: 양식 1회 파싱 + JSON N회 채움 (D21)
//!
//! 양식을 한 번 파싱하여 Document를 얻고, 각 JSON 항목마다 Document를 clone하여 치환 후
//! 직렬화한다. 페이지네이션 비용을 파싱 단계에서만 한 번 지불한다 (D18).

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

use rhwp::parser::parse_document;
use rhwp::serializer::serialize_hwp;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::cli::OnMissingKey;
use crate::error::BatchError;
use crate::template::fill_document;

// ── 매니페스트 ────────────────────────────────────────────────────────────────

/// 배치 매니페스트: 여러 항목을 한 번에 처리하는 작업 명세
#[derive(Debug, Deserialize)]
pub struct BatchManifest {
    pub items: Vec<BatchItem>,
}

#[derive(Debug, Deserialize)]
pub struct BatchItem {
    /// 데이터 JSON 파일 경로 (매니페스트 기준 상대 경로)
    pub data: String,
    /// 출력 HWP 파일 경로
    pub output: String,
}

// ── 결과 보고 ─────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct BatchReport {
    pub total: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub items: Vec<BatchItemReport>,
}

#[derive(Debug, Serialize)]
pub struct BatchItemReport {
    pub data: String,
    pub output: String,
    pub status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

// ── 배치 실행 ─────────────────────────────────────────────────────────────────

pub struct BatchConfig {
    pub on_missing_key: OnMissingKey,
    pub on_error_continue: bool,
    pub threads: usize,
    pub overwrite: bool,
}

/// 배치 실행 진입점.
///
/// template_path에서 양식을 1회 파싱하고, items를 병렬로 처리한다.
/// 반환값: (exit_code, report)
pub fn run_batch(
    template_path: &Path,
    items: &[(String, String)], // (data_path, output_path) pairs
    cfg: &BatchConfig,
) -> (i32, BatchReport) {
    // 1. 양식 1회 파싱
    let template_bytes = match std::fs::read(template_path) {
        Ok(b) => b,
        Err(e) => {
            let report = BatchReport {
                total: items.len(),
                succeeded: 0,
                failed: items.len(),
                items: items.iter().map(|(d, o)| BatchItemReport {
                    data: d.clone(),
                    output: o.clone(),
                    status: "failed".into(),
                    error: Some(e.to_string()),
                }).collect(),
            };
            return (1, report);
        }
    };
    let template_doc = match parse_document(&template_bytes) {
        Ok(d) => d,
        Err(e) => {
            let report = BatchReport {
                total: items.len(),
                succeeded: 0,
                failed: items.len(),
                items: items.iter().map(|(d, o)| BatchItemReport {
                    data: d.clone(),
                    output: o.clone(),
                    status: "failed".into(),
                    error: Some(format!("{:?}", e)),
                }).collect(),
            };
            return (1, report);
        }
    };

    let template_doc = Arc::new(template_doc);
    let results: Arc<Mutex<Vec<BatchItemReport>>> = Arc::new(Mutex::new(Vec::new()));
    let abort: Arc<AtomicBool> = Arc::new(AtomicBool::new(false));
    let threads = cfg.threads.max(1);
    let chunk_size = items.len().div_ceil(threads).max(1);

    // 2. N개 항목을 workers에 분배하여 병렬 처리
    let mut handles = Vec::new();
    for chunk in items.chunks(chunk_size) {
        let chunk: Vec<_> = chunk.to_vec();
        let template_doc = Arc::clone(&template_doc);
        let results = Arc::clone(&results);
        let abort = Arc::clone(&abort);
        let on_missing_key = cfg.on_missing_key;
        let on_error_continue = cfg.on_error_continue;
        let overwrite = cfg.overwrite;

        let handle = thread::spawn(move || {
            for (data_path, output_path) in &chunk {
                if abort.load(Ordering::Relaxed) {
                    break;
                }
                let item_result = process_one(
                    &template_doc,
                    data_path,
                    output_path,
                    on_missing_key,
                    overwrite,
                );
                let failed = item_result.is_err();
                let report_item = match item_result {
                    Ok(_) => BatchItemReport {
                        data: data_path.clone(),
                        output: output_path.clone(),
                        status: "ok".into(),
                        error: None,
                    },
                    Err(e) => {
                        let msg = e.to_string();
                        tracing::error!(data = %data_path, error = %msg, "batch item failed");
                        BatchItemReport {
                            data: data_path.clone(),
                            output: output_path.clone(),
                            status: "failed".into(),
                            error: Some(msg),
                        }
                    }
                };
                results.lock().unwrap().push(report_item);
                if failed && !on_error_continue {
                    abort.store(true, Ordering::Relaxed);
                    break;
                }
            }
        });
        handles.push(handle);
    }
    for h in handles { let _ = h.join(); }

    let items_report = Arc::try_unwrap(results).unwrap().into_inner().unwrap();
    let succeeded = items_report.iter().filter(|r| r.status == "ok").count();
    let failed = items_report.iter().filter(|r| r.status == "failed").count();
    let exit_code = if failed == 0 { 0 } else if succeeded > 0 { 4 } else { 1 };

    let report = BatchReport {
        total: items.len(),
        succeeded,
        failed,
        items: items_report,
    };
    (exit_code, report)
}

fn process_one(
    template_doc: &rhwp::model::document::Document,
    data_path: &str,
    output_path: &str,
    on_missing_key: OnMissingKey,
    overwrite: bool,
) -> Result<(), BatchError> {
    let out = std::path::Path::new(output_path);
    if out.exists() && !overwrite {
        return Err(BatchError::OutputExists(output_path.to_string()));
    }

    let json_str = std::fs::read_to_string(data_path)?;
    let data: Value = serde_json::from_str(&json_str)?;

    // Clone template IR for this item
    let doc_clone = template_doc.clone();
    let filled = fill_document(doc_clone, &data, on_missing_key)?;
    let bytes = serialize_hwp(&filled)
        .map_err(|e| BatchError::Serialize(format!("{:?}", e)))?;

    if let Some(parent) = out.parent() {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent)?;
        }
    }
    std::fs::write(out, bytes)?;
    tracing::info!(output = %output_path, "batch item written");
    Ok(())
}

// ── 디렉토리 모드 헬퍼 ───────────────────────────────────────────────────────

/// --data-dir 모드: 데이터 디렉토리의 모든 JSON 파일을 items로 변환
pub fn items_from_data_dir(
    data_dir: &Path,
    output_dir: &Path,
) -> Result<Vec<(String, String)>, BatchError> {
    let mut items = Vec::new();
    for entry in walkdir::WalkDir::new(data_dir)
        .follow_links(true)
        .into_iter()
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().is_file())
    {
        let path = entry.path();
        let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
        if ext != "json" { continue; }
        let rel = path.strip_prefix(data_dir).unwrap_or(path);
        let out = output_dir.join(rel.with_extension("hwp"));
        items.push((
            path.display().to_string(),
            out.display().to_string(),
        ));
    }
    Ok(items)
}

/// --manifest 모드: 매니페스트 JSON을 파싱하여 items 반환
pub fn items_from_manifest(
    manifest_path: &Path,
) -> Result<Vec<(String, String)>, BatchError> {
    let json_str = std::fs::read_to_string(manifest_path)?;
    let manifest: BatchManifest = serde_json::from_str(&json_str)?;
    let base = manifest_path.parent().unwrap_or(Path::new("."));
    Ok(manifest.items.iter().map(|item| {
        let data = base.join(&item.data).display().to_string();
        (data, item.output.clone())
    }).collect())
}
