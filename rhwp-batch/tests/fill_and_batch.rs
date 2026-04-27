//! fill·batch 통합 smoke test
//!
//! 외부 작성 양식 HWP 없이도 회귀를 잡기 위해, 기존 샘플을 파싱하여
//! 첫 번째 비-빈 paragraph의 텍스트에 마커를 주입한 뒤 fill 흐름을 검증한다.
//! 배치 smoke는 같은 방식으로 만든 "가짜 양식"을 임시 디렉토리에 직렬화하여
//! `run_batch`가 N건을 모두 처리하고 리포트를 일관되게 산출하는지를 본다.

use std::fs;
use std::io::Write;

use rhwp::model::document::Document;
use rhwp::parser::parse_hwp;
use rhwp::serializer::serialize_hwp;

use rhwp_batch::batch::{run_batch, BatchConfig};
use rhwp_batch::cli::OnMissingKey;
use rhwp_batch::template::fill_document;

const SAMPLE: &str = "../rhwp/samples/basic/english.hwp";

/// 첫 번째 비-빈 paragraph의 텍스트를 주어진 값으로 치환.
/// raw_stream을 비워 직렬화가 model을 다시 작성하게 한다.
fn inject_template_text(doc: &mut Document, replacement: &str) -> bool {
    for section in &mut doc.sections {
        let mut injected = false;
        for para in &mut section.paragraphs {
            if !injected && !para.text.trim().is_empty() {
                para.text = replacement.to_string();
                para.char_shapes.clear();
                para.line_segs.clear();
                injected = true;
            }
        }
        if injected {
            section.raw_stream = None;
            return true;
        }
    }
    false
}

fn parse_sample() -> Document {
    let bytes = fs::read(SAMPLE).expect("sample read");
    parse_hwp(&bytes).expect("sample parse")
}

#[test]
fn fill_replaces_simple_marker() {
    let mut doc = parse_sample();
    assert!(
        inject_template_text(&mut doc, "안녕 {{name}}, 나이 {{age}}세"),
        "no usable paragraph found in sample"
    );

    let data = serde_json::json!({ "name": "홍길동", "age": 30 });
    let filled = fill_document(doc, &data, OnMissingKey::Error).expect("fill ok");

    let mut found = false;
    for section in &filled.sections {
        for para in &section.paragraphs {
            if para.text.contains("홍길동") && para.text.contains("30") {
                assert!(!para.text.contains("{{"), "marker leftover: {:?}", para.text);
                found = true;
            }
        }
    }
    assert!(found, "filled text not found");
}

#[test]
fn fill_missing_key_error_propagates() {
    let mut doc = parse_sample();
    inject_template_text(&mut doc, "값: {{nonexistent.path}}");

    let data = serde_json::json!({});
    let err = fill_document(doc, &data, OnMissingKey::Error);
    assert!(err.is_err(), "missing key should error");
}

#[test]
fn fill_missing_key_empty_blanks() {
    let mut doc = parse_sample();
    inject_template_text(&mut doc, "값: [{{nonexistent}}]");

    let filled = fill_document(doc, &serde_json::json!({}), OnMissingKey::Empty)
        .expect("empty mode should succeed");

    let texts: Vec<&str> = filled
        .sections
        .iter()
        .flat_map(|s| s.paragraphs.iter())
        .map(|p| p.text.as_str())
        .collect();
    assert!(
        texts.iter().any(|t| t.contains("값: []")),
        "expected blanked marker, got: {:?}",
        texts
    );
}

#[test]
fn fill_missing_key_keep_preserves() {
    let mut doc = parse_sample();
    inject_template_text(&mut doc, "값: {{missing}}");

    let filled = fill_document(doc, &serde_json::json!({}), OnMissingKey::Keep)
        .expect("keep mode should succeed");
    let any_kept = filled.sections.iter().any(|s| {
        s.paragraphs.iter().any(|p| p.text.contains("{{missing}}"))
    });
    assert!(any_kept, "keep mode should preserve marker text");
}

/// "가짜 양식"을 직렬화하여 임시 디렉토리에 저장하고, JSON N건과 함께 run_batch 호출.
#[test]
fn batch_runs_multiple_items_and_reports() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let template_path = tmp.path().join("template.hwp");
    let output_dir = tmp.path().join("out");
    fs::create_dir_all(&output_dir).unwrap();

    // 1) 샘플 파싱 → 마커 주입 → 직렬화하여 양식 파일 작성
    let mut doc = parse_sample();
    assert!(inject_template_text(&mut doc, "주문자: {{name}}"));
    let bytes = serialize_hwp(&doc).expect("serialize template");
    fs::write(&template_path, &bytes).unwrap();

    // 2) JSON 데이터 3건 생성
    let mut items: Vec<(String, String)> = Vec::new();
    for i in 0..3 {
        let data_path = tmp.path().join(format!("data_{}.json", i));
        let mut f = fs::File::create(&data_path).unwrap();
        writeln!(f, r#"{{ "name": "고객-{}" }}"#, i).unwrap();
        let out_path = output_dir.join(format!("out_{}.hwp", i));
        items.push((
            data_path.display().to_string(),
            out_path.display().to_string(),
        ));
    }

    // 3) run_batch 실행
    let cfg = BatchConfig {
        on_missing_key: OnMissingKey::Error,
        on_error_continue: false,
        threads: 2,
        overwrite: false,
    };
    let (exit_code, report) = run_batch(&template_path, &items, &cfg);

    assert_eq!(exit_code, 0, "expected success exit, got {}", exit_code);
    assert_eq!(report.total, 3);
    assert_eq!(report.succeeded, 3);
    assert_eq!(report.failed, 0);

    // 4) 각 출력 파일이 실제로 존재하고 비어있지 않은지 확인
    for (_, out) in &items {
        let meta = fs::metadata(out).expect("output file");
        assert!(meta.len() > 0, "output {} is empty", out);
    }
}

#[test]
fn batch_partial_failure_yields_exit_4() {
    let tmp = tempfile::tempdir().unwrap();
    let template_path = tmp.path().join("template.hwp");
    let output_dir = tmp.path().join("out");
    fs::create_dir_all(&output_dir).unwrap();

    let mut doc = parse_sample();
    assert!(inject_template_text(&mut doc, "값: {{required}}"));
    fs::write(&template_path, serialize_hwp(&doc).unwrap()).unwrap();

    // 한 건은 키 누락 (실패), 두 건은 정상
    let make = |i: usize, body: &str| {
        let dp = tmp.path().join(format!("d_{}.json", i));
        fs::write(&dp, body).unwrap();
        let op = output_dir.join(format!("o_{}.hwp", i));
        (dp.display().to_string(), op.display().to_string())
    };
    let items = vec![
        make(0, r#"{"required": "ok"}"#),
        make(1, r#"{}"#), // missing → fail
        make(2, r#"{"required": "ok"}"#),
    ];

    let cfg = BatchConfig {
        on_missing_key: OnMissingKey::Error,
        on_error_continue: true,
        threads: 1,
        overwrite: false,
    };
    let (exit_code, report) = run_batch(&template_path, &items, &cfg);

    assert_eq!(exit_code, 4, "partial failure should exit 4");
    assert_eq!(report.total, 3);
    assert_eq!(report.succeeded, 2);
    assert_eq!(report.failed, 1);
}
