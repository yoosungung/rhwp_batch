//! 양식 마커 파싱 및 치환 (M3 — 텍스트 전용, M4 — 표·이미지 확장)
//!
//! 마커 문법 (D3):
//!   `{{key}}`          — 단순 값 치환 (JMESPath 표현식)
//!   `{{table:expr}}`   — 표 행 확장 (M4)
//!   `{{image:key}}`    — 이미지 삽입 (M4)

use rhwp::model::control::Control;
use rhwp::model::document::Document;
use rhwp::model::paragraph::{CharShapeRef, Paragraph};
use serde_json::Value;

use crate::cli::OnMissingKey;
use crate::error::BatchError;

// ── 공개 진입점 ───────────────────────────────────────────────────────────────

/// 양식 문서에서 `{{...}}` 마커를 data JSON 값으로 치환하여 새 Document 반환.
pub fn fill_document(
    mut doc: Document,
    data: &Value,
    on_missing: OnMissingKey,
) -> Result<Document, BatchError> {
    for section in &mut doc.sections {
        // raw_stream 무효화 → serialize_hwp이 model에서 재직렬화
        section.raw_stream = None;
        fill_paragraphs(&mut section.paragraphs, data, on_missing)?;
    }
    Ok(doc)
}

// ── 표 행 확장 (M4 Stage 2-3) ────────────────────────────────────────────────

/// 표 행을 `{{table:expr}}` 마커를 기준으로 동적 확장한다.
/// fill_document 내 fill_paragraphs → fill_paragraph 에서 Table 컨트롤 처리 전 호출된다.
fn expand_table(
    table: &mut rhwp::model::table::Table,
    data: &Value,
) -> Result<(), BatchError> {
    let col_count = table.col_count as usize;
    if col_count == 0 { return Ok(()); }

    // template row 탐색: table markers를 포함한 행 번호 찾기
    let mut template_row: Option<usize> = None;
    let mut array_expr: Option<String> = None;

    'outer: for cell in &table.cells {
        let row = cell.row as usize;
        for para in &cell.paragraphs {
            for m in find_markers(&para.text) {
                if let MarkerKind::Table(expr) = m.kind {
                    template_row = Some(row);
                    array_expr = Some(strip_field(&expr));
                    break 'outer;
                }
            }
        }
    }

    let (template_row, array_expr) = match (template_row, array_expr) {
        (Some(r), Some(e)) => (r, e),
        _ => return Ok(()), // no table markers
    };

    // 배열 값 평가
    let compiled = jmespath::compile(&array_expr)
        .map_err(|e| BatchError::Template(format!("invalid table JMESPath '{}': {}", array_expr, e)))?;
    let result = compiled
        .search(data.clone())
        .map_err(|e| BatchError::Template(format!("table JMESPath eval error '{}': {}", array_expr, e)))?;

    let items: Vec<Value> = match &*result {
        jmespath::Variable::Array(arr) => arr.iter().map(|v| {
            serde_json::to_value(v.as_ref()).unwrap_or(Value::Null)
        }).collect(),
        _ => return Ok(()), // not an array → skip expansion
    };

    if items.is_empty() {
        // Remove the template row entirely
        table.cells.retain(|c| c.row as usize != template_row);
        rebuild_grid(table);
        return Ok(());
    }

    // Collect template row cells (cloned)
    let template_cells: Vec<rhwp::model::table::Cell> = table.cells.iter()
        .filter(|c| c.row as usize == template_row)
        .cloned()
        .collect();

    // Remove template row from cells
    table.cells.retain(|c| c.row as usize != template_row);

    // Shift rows after template_row by (items.len() - 1)
    let row_delta = items.len() as i64 - 1;
    if row_delta != 0 {
        for c in &mut table.cells {
            if c.row as usize > template_row {
                c.row = ((c.row as i64) + row_delta) as u16;
            }
        }
    }

    // Insert expanded rows in place of template row
    for (i, item) in items.iter().enumerate() {
        let new_row = (template_row + i) as u16;
        for tmpl_cell in &template_cells {
            let mut new_cell = tmpl_cell.clone();
            new_cell.row = new_row;
            // Fill markers in this cell with item data
            for para in &mut new_cell.paragraphs {
                substitute_table_row(para, item);
            }
            table.cells.push(new_cell);
        }
    }

    // Sort cells by row then col for consistency
    table.cells.sort_by_key(|c| (c.row, c.col));

    // Update row count
    table.row_count = ((table.row_count as i64 + row_delta) as u16).max(1);

    rebuild_grid(table);
    Ok(())
}

/// `{{table:expr.field}}` を通常の `{{field}}` マーカーに変換して文字列置換（行展開後の単セル用）
fn substitute_table_row(para: &mut Paragraph, item: &Value) {
    let markers = find_markers(&para.text);
    if markers.is_empty() { return; }

    let mut new_text = para.text.clone();
    let mut char_shape_deltas: Vec<(usize, i64)> = Vec::new();

    for marker in markers.iter().rev() {
        let replacement = if let MarkerKind::Table(expr) = &marker.kind {
            // expr = "items[].fieldname" → field = last segment
            let field = last_field(expr);
            match item.get(field) {
                Some(v) => jmes_value_to_string_from_json(v),
                None => String::new(),
            }
        } else {
            continue;
        };

        let old_len_utf16 = char_utf16_len(&new_text[marker.byte_start..marker.byte_end]);
        let new_len_utf16 = char_utf16_len(&replacement);
        let delta: i64 = new_len_utf16 as i64 - old_len_utf16 as i64;
        let prefix_utf16 = char_utf16_len(&new_text[..marker.byte_start]);

        new_text.replace_range(marker.byte_start..marker.byte_end, &replacement);
        if delta != 0 {
            char_shape_deltas.push((prefix_utf16 + old_len_utf16, delta));
        }
    }

    adjust_char_shapes(&mut para.char_shapes, &char_shape_deltas);
    para.line_segs.clear();
    para.text = new_text;
}

fn strip_field(expr: &str) -> String {
    // "items[].name" → "items[]" / "rows.items[].name" → "rows.items[]"
    // Strip the last ".fieldname" after "[]"
    if let Some(idx) = expr.rfind('.') {
        let before = &expr[..idx];
        if before.ends_with(']') {
            return before.to_string();
        }
    }
    // If no "[]" pattern, try to use the expression as-is (simple array key)
    expr.to_string()
}

fn last_field(expr: &str) -> &str {
    // "items[].name" → "name"
    expr.rsplit('.').next().unwrap_or(expr)
}

fn jmes_value_to_string_from_json(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Number(n) => n.to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

fn rebuild_grid(table: &mut rhwp::model::table::Table) {
    let row_count = table.row_count as usize;
    let col_count = table.col_count as usize;
    let mut grid = vec![None; row_count * col_count];
    for (cell_idx, cell) in table.cells.iter().enumerate() {
        let r = cell.row as usize;
        let c = cell.col as usize;
        let rs = cell.row_span as usize;
        let cs = cell.col_span as usize;
        for dr in 0..rs {
            for dc in 0..cs {
                let gr = r + dr;
                let gc = c + dc;
                if gr < row_count && gc < col_count {
                    grid[gr * col_count + gc] = Some(cell_idx);
                }
            }
        }
    }
    table.cell_grid = grid;
}

// ── 문단 재귀 처리 ────────────────────────────────────────────────────────────

fn fill_paragraphs(
    paragraphs: &mut Vec<Paragraph>,
    data: &Value,
    on_missing: OnMissingKey,
) -> Result<(), BatchError> {
    for para in paragraphs.iter_mut() {
        fill_paragraph(para, data, on_missing)?;

        // 컨트롤 내부(표 셀·머리말·꼬리말·각주) 재귀 처리
        for ctrl in para.controls.iter_mut() {
            match ctrl {
                Control::Table(table) => {
                    // 표 행 확장 먼저 (M4)
                    expand_table(table, data)?;
                    // 이후 일반 마커 치환
                    for cell in table.cells.iter_mut() {
                        fill_paragraphs(&mut cell.paragraphs, data, on_missing)?;
                    }
                }
                Control::Header(hdr) => {
                    fill_paragraphs(&mut hdr.paragraphs, data, on_missing)?;
                }
                Control::Footer(ftr) => {
                    fill_paragraphs(&mut ftr.paragraphs, data, on_missing)?;
                }
                Control::Footnote(fn_) => {
                    fill_paragraphs(&mut fn_.paragraphs, data, on_missing)?;
                }
                Control::Endnote(en) => {
                    fill_paragraphs(&mut en.paragraphs, data, on_missing)?;
                }
                _ => {}
            }
        }
    }
    Ok(())
}

// ── 단일 문단 치환 ────────────────────────────────────────────────────────────

fn fill_paragraph(
    para: &mut Paragraph,
    data: &Value,
    on_missing: OnMissingKey,
) -> Result<(), BatchError> {
    // {{table:...}} 또는 {{image:...}} 마커는 M4에서 처리
    // 여기서는 단순 {{key}} (table:/image: 접두어 없는) 만 처리
    let markers = find_markers(&para.text);
    if markers.is_empty() {
        return Ok(());
    }

    // 뒤에서 앞으로 치환 (인덱스 안정성 유지)
    let mut new_text = para.text.clone();
    let mut char_shape_deltas: Vec<(usize, i64)> = Vec::new(); // (utf16_pos, delta)

    for marker in markers.iter().rev() {
        match &marker.kind {
            MarkerKind::Table(_) | MarkerKind::Image(_) => continue, // M4
            MarkerKind::Simple(expr) => {
                let replacement = resolve_value(expr, data, on_missing)?;
                let old_len_utf16 = char_utf16_len(&new_text[marker.byte_start..marker.byte_end]);
                let new_len_utf16 = char_utf16_len(&replacement);
                let delta: i64 = new_len_utf16 as i64 - old_len_utf16 as i64;

                // 마커 앞 텍스트의 UTF-16 길이 (char_shapes 조정 기준점)
                let prefix_utf16 = char_utf16_len(&new_text[..marker.byte_start]);

                new_text.replace_range(marker.byte_start..marker.byte_end, &replacement);

                if delta != 0 {
                    char_shape_deltas.push((prefix_utf16 + old_len_utf16, delta));
                }
            }
        }
    }

    // char_shapes 위치 조정
    adjust_char_shapes(&mut para.char_shapes, &char_shape_deltas);

    // line_segs 무효화 (뷰어가 재계산)
    para.line_segs.clear();

    para.text = new_text;
    Ok(())
}

// ── 마커 파싱 ─────────────────────────────────────────────────────────────────

#[derive(Debug)]
struct Marker {
    /// 바이트 기준 시작 (inclusive)
    byte_start: usize,
    /// 바이트 기준 끝 (exclusive)
    byte_end: usize,
    kind: MarkerKind,
}

#[derive(Debug)]
#[allow(dead_code)]
enum MarkerKind {
    Simple(String),
    Table(String),
    Image(String),
}

fn find_markers(text: &str) -> Vec<Marker> {
    let mut markers = Vec::new();
    let bytes = text.as_bytes();
    let len = bytes.len();
    let mut i = 0;
    while i + 1 < len {
        if bytes[i] == b'{' && bytes[i + 1] == b'{' {
            // search for closing }}
            if let Some(rel) = find_close(&bytes[i + 2..]) {
                let inner_start = i + 2;
                let inner_end = inner_start + rel;
                let end = inner_end + 2; // skip }}
                let inner = &text[inner_start..inner_end];
                let kind = if let Some(expr) = inner.strip_prefix("table:") {
                    MarkerKind::Table(expr.trim().to_string())
                } else if let Some(key) = inner.strip_prefix("image:") {
                    MarkerKind::Image(key.trim().to_string())
                } else {
                    MarkerKind::Simple(inner.trim().to_string())
                };
                markers.push(Marker { byte_start: i, byte_end: end, kind });
                i = end;
                continue;
            }
        }
        i += 1;
    }
    markers
}

fn find_close(bytes: &[u8]) -> Option<usize> {
    let mut i = 0;
    while i + 1 < bytes.len() {
        if bytes[i] == b'}' && bytes[i + 1] == b'}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

// ── JMESPath 평가 ─────────────────────────────────────────────────────────────

fn resolve_value(expr: &str, data: &Value, on_missing: OnMissingKey) -> Result<String, BatchError> {
    if expr.is_empty() {
        return Ok(String::new());
    }
    let compiled = jmespath::compile(expr)
        .map_err(|e| BatchError::Template(format!("invalid JMESPath '{}': {}", expr, e)))?;
    let result = compiled
        .search(data.clone())
        .map_err(|e| BatchError::Template(format!("JMESPath eval error '{}': {}", expr, e)))?;

    match &*result {
        jmespath::Variable::Null => {
            match on_missing {
                OnMissingKey::Error => Err(BatchError::MissingKey(expr.to_string())),
                OnMissingKey::Empty => Ok(String::new()),
                OnMissingKey::Keep => Ok(format!("{{{{{}}}}}", expr)),
            }
        }
        v => Ok(jmes_value_to_string(v)),
    }
}

fn jmes_value_to_string(v: &jmespath::Variable) -> String {
    match v {
        jmespath::Variable::String(s) => s.clone(),
        jmespath::Variable::Number(n) => n.to_string(),
        jmespath::Variable::Bool(b) => b.to_string(),
        jmespath::Variable::Null => String::new(),
        other => other.to_string(),
    }
}

// ── UTF-16 유틸 ───────────────────────────────────────────────────────────────

fn char_utf16_len(s: &str) -> usize {
    s.chars().map(|c| if (c as u32) > 0xFFFF { 2 } else { 1 }).sum()
}

/// char_shapes의 start_pos(UTF-16 기준)를 조정한다.
/// `deltas`: (threshold_utf16, delta) 목록. threshold 이상인 start_pos에 delta를 누적 적용.
fn adjust_char_shapes(char_shapes: &mut Vec<CharShapeRef>, deltas: &[(usize, i64)]) {
    if deltas.is_empty() || char_shapes.is_empty() {
        return;
    }
    for cs in char_shapes.iter_mut() {
        let mut cumulative: i64 = 0;
        for &(threshold, delta) in deltas {
            if cs.start_pos as usize >= threshold {
                cumulative += delta;
            }
        }
        if cumulative != 0 {
            cs.start_pos = (cs.start_pos as i64 + cumulative).max(0) as u32;
        }
    }
}

// ── 표 확장 (M4 진입점 — 현재 stub) ──────────────────────────────────────────

/// 문서 내 모든 표를 동적 확장한다 (외부 호출용).
/// fill_document 내에서 자동 호출되므로 별도 호출 불필요.
pub fn expand_table_markers(
    doc: &mut Document,
    data: &Value,
) -> Result<(), BatchError> {
    for section in &mut doc.sections {
        for para in &mut section.paragraphs {
            for ctrl in &mut para.controls {
                if let Control::Table(table) = ctrl {
                    expand_table(table, data)?;
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn find_markers_simple() {
        let text = "안녕 {{name}}, 나이: {{age}}세";
        let markers = find_markers(text);
        assert_eq!(markers.len(), 2);
        assert!(matches!(&markers[0].kind, MarkerKind::Simple(e) if e == "name"));
        assert!(matches!(&markers[1].kind, MarkerKind::Simple(e) if e == "age"));
    }

    #[test]
    fn find_markers_prefix() {
        let text = "{{table:rows.items[].name}} {{image:logo}}";
        let markers = find_markers(text);
        assert_eq!(markers.len(), 2);
        assert!(matches!(&markers[0].kind, MarkerKind::Table(_)));
        assert!(matches!(&markers[1].kind, MarkerKind::Image(_)));
    }

    #[test]
    fn resolve_simple_key() {
        let data = serde_json::json!({ "name": "홍길동" });
        let result = resolve_value("name", &data, OnMissingKey::Error).unwrap();
        assert_eq!(result, "홍길동");
    }

    #[test]
    fn resolve_missing_key_error() {
        let data = serde_json::json!({});
        let err = resolve_value("missing", &data, OnMissingKey::Error);
        assert!(err.is_err());
    }

    #[test]
    fn resolve_missing_key_empty() {
        let data = serde_json::json!({});
        let result = resolve_value("missing", &data, OnMissingKey::Empty).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn resolve_missing_key_keep() {
        let data = serde_json::json!({});
        let result = resolve_value("missing", &data, OnMissingKey::Keep).unwrap();
        assert_eq!(result, "{{missing}}");
    }

    #[test]
    fn utf16_len_ascii() {
        assert_eq!(char_utf16_len("hello"), 5);
    }

    #[test]
    fn utf16_len_korean() {
        // 한글 각 글자 = 1 UTF-16 코드 유닛
        assert_eq!(char_utf16_len("안녕"), 2);
    }
}
