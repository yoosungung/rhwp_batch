use rhwp::parser::parse_hwp;
use rhwp_batch::adapter::{convert, ConvertOptions};
use rhwp_batch::dto::BlockJson;

fn sample(name: &str) -> std::path::PathBuf {
    let base = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
    base.join("samples/from_rhwp").join(name)
}

fn parse_sample(name: &str) -> rhwp::model::document::Document {
    let path = sample(name);
    let data = std::fs::read(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {}", path.display(), e));
    parse_hwp(&data).unwrap_or_else(|e| panic!("parse failed {}: {:?}", name, e))
}

fn default_opts(name: &str) -> ConvertOptions {
    ConvertOptions {
        source_filename: name.to_string(),
        source_format: "hwp".to_string(),
        source_sha256: "test".to_string(),
        ..Default::default()
    }
}

#[test]
fn convert_table_sample() {
    let doc = parse_sample("table-001.hwp");
    let opts = default_opts("table-001.hwp");
    let result = convert(&doc, &opts);

    assert_eq!(result.schema_version, "1.0.0");
    assert_eq!(result.source.format, "hwp");
    assert!(!result.blocks.is_empty(), "no blocks produced");

    let tables: Vec<_> = result
        .blocks
        .iter()
        .filter(|b| matches!(b, BlockJson::Table { .. }))
        .collect();
    assert!(!tables.is_empty(), "expected at least one table block");

    if let BlockJson::Table { markdown, headers, .. } = &tables[0] {
        assert!(markdown.contains('|'), "markdown should be a table");
        assert!(!headers.is_empty(), "headers should not be empty");
    }
}

#[test]
fn convert_produces_nfc_text() {
    use unicode_normalization::UnicodeNormalization;
    let doc = parse_sample("table-001.hwp");
    let opts = default_opts("table-001.hwp");
    let result = convert(&doc, &opts);

    for block in &result.blocks {
        let text = match block {
            BlockJson::Paragraph { text, .. }
            | BlockJson::Heading { text, .. }
            | BlockJson::ListItem { text, .. } => Some(text.as_str()),
            _ => None,
        };
        if let Some(t) = text {
            let nfc: String = t.nfc().collect();
            assert_eq!(t, &nfc, "text is not NFC normalized");
        }
    }
}

#[test]
fn convert_footnote_sample() {
    let doc = parse_sample("footnote-01.hwp");
    let opts = default_opts("footnote-01.hwp");
    let result = convert(&doc, &opts);

    assert!(!result.blocks.is_empty());
    let footnotes: Vec<_> = result
        .blocks
        .iter()
        .filter(|b| matches!(b, BlockJson::Footnote { .. }))
        .collect();
    assert!(!footnotes.is_empty(), "expected at least one footnote block");
}

#[test]
fn convert_image_sample() {
    let doc = parse_sample("hwp-img-001.hwp");
    let opts = ConvertOptions {
        source_filename: "hwp-img-001.hwp".to_string(),
        source_format: "hwp".to_string(),
        source_sha256: "test".to_string(),
        image_mode: rhwp_batch::adapter::ImageMode::Inline,
        ..Default::default()
    };
    let result = convert(&doc, &opts);

    assert!(!result.blocks.is_empty());
    let images: Vec<_> = result
        .blocks
        .iter()
        .filter(|b| matches!(b, BlockJson::Image { .. }))
        .collect();
    // 이미지 샘플이면 반드시 이미지 블록 존재
    if !images.is_empty() {
        if let BlockJson::Image { asset_ref, width_mm, height_mm, .. } = &images[0] {
            assert!(!asset_ref.is_empty());
            assert!(*width_mm > 0.0);
            assert!(*height_mm > 0.0);
            let asset = result.assets.get(asset_ref).expect("asset not registered");
            // inline 모드: data_base64 있어야 함
            assert!(asset.data_base64.is_some() || asset.path.is_some());
        }
    }
}

#[test]
fn convert_block_ids_are_unique() {
    let doc = parse_sample("table-001.hwp");
    let opts = default_opts("table-001.hwp");
    let result = convert(&doc, &opts);

    let mut ids = std::collections::HashSet::new();
    for block in &result.blocks {
        let id = match block {
            BlockJson::Paragraph { id, .. }
            | BlockJson::Heading { id, .. }
            | BlockJson::Table { id, .. }
            | BlockJson::Image { id, .. }
            | BlockJson::ListItem { id, .. }
            | BlockJson::Header { id, .. }
            | BlockJson::Footer { id, .. }
            | BlockJson::Footnote { id, .. }
            | BlockJson::Caption { id, .. } => id,
        };
        assert!(ids.insert(id.as_str()), "duplicate block id: {}", id);
    }
}

#[test]
fn convert_source_metadata() {
    let path = sample("table-001.hwp");
    let data = std::fs::read(&path).unwrap();
    let sha256 = rhwp_batch::adapter::sha256_file(&data);
    let doc = parse_hwp(&data).unwrap();
    let opts = ConvertOptions {
        source_filename: "table-001.hwp".to_string(),
        source_format: "hwp".to_string(),
        source_sha256: sha256.clone(),
        ..Default::default()
    };
    let result = convert(&doc, &opts);
    assert_eq!(result.source.filename, "table-001.hwp");
    assert_eq!(result.source.sha256, sha256);
    assert!(result.source.section_count > 0);
    assert!(!result.source.extracted_at.is_empty());
}
