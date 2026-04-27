use std::collections::HashMap;
use std::path::PathBuf;

use base64::{engine::general_purpose::STANDARD as BASE64, Engine as _};
use sha2::{Digest, Sha256};
use unicode_normalization::UnicodeNormalization;

use rhwp::model::control::Control;
use rhwp::model::document::Document;
use rhwp::model::paragraph::Paragraph;

use crate::dto::{AssetJson, BlockJson, DocumentJson, MergedCell, SourceJson};

/// HwpUnit (u32, 1/7200 인치) → 밀리미터
fn hwp_unit_to_mm(u: u32) -> f64 {
    u as f64 / 7200.0 * 25.4
}

/// style local_name에서 heading level 추출 (D23 휴리스틱)
fn heading_level(name: &str, style_map: &HashMap<String, u8>) -> Option<u8> {
    let name = name.trim();
    if let Some(&lvl) = style_map.get(name) {
        return Some(lvl);
    }
    for prefix in &["개요 ", "개요", "제목 ", "제목", "Heading ", "heading "] {
        if let Some(rest) = name.strip_prefix(prefix) {
            if let Ok(n) = rest.trim().parse::<u8>() {
                if (1..=6).contains(&n) {
                    return Some(n);
                }
            }
        }
    }
    None
}

/// 문단 텍스트에서 제어 문자 제거 후 NFC 정규화 (D28)
fn clean_text(raw: &str) -> String {
    raw.chars()
        .filter(|&c| !c.is_control() || c == '\n' || c == '\t')
        .collect::<String>()
        .nfc()
        .collect()
}

/// 셀 내 모든 문단 텍스트를 <br>로 결합 (D26)
fn cell_text(paragraphs: &[Paragraph]) -> String {
    paragraphs
        .iter()
        .map(|p| clean_text(&p.text))
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join("<br>")
}

/// 확장자 → MIME 타입
fn mime_from_ext(ext: &str) -> &'static str {
    match ext.to_ascii_lowercase().as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "bmp" => "image/bmp",
        "gif" => "image/gif",
        "svg" => "image/svg+xml",
        "wmf" => "image/wmf",
        "emf" => "image/emf",
        _ => "application/octet-stream",
    }
}

fn sha256_hex(data: &[u8]) -> String {
    let mut h = Sha256::new();
    h.update(data);
    hex::encode(h.finalize())
}

fn current_iso8601() -> String {
    chrono::Utc::now().format("%Y-%m-%dT%H:%M:%SZ").to_string()
}

/// 이미지 모드 (D4)
#[derive(Debug, Clone, PartialEq)]
pub enum ImageMode {
    /// 별도 파일로 추출 (기본)
    Extract,
    /// base64 인라인
    Inline,
}

pub struct ConvertOptions {
    /// 원본 파일명 (source.filename)
    pub source_filename: String,
    /// "hwp" 또는 "hwpx"
    pub source_format: String,
    /// 원본 파일 sha256 (source.sha256)
    pub source_sha256: String,
    /// 이미지 모드 (D4 기본 Extract)
    pub image_mode: ImageMode,
    /// Extract 모드에서 이미지 저장 디렉토리. None이면 "<output_dir>/<stem>.assets/"
    pub image_dir: Option<PathBuf>,
    /// heading 스타일 이름 → level 오버라이드 (D23)
    pub heading_style_map: HashMap<String, u8>,
}

impl Default for ConvertOptions {
    fn default() -> Self {
        ConvertOptions {
            source_filename: String::new(),
            source_format: "hwp".to_string(),
            source_sha256: String::new(),
            image_mode: ImageMode::Extract,
            image_dir: None,
            heading_style_map: HashMap::new(),
        }
    }
}

// ─────────────────────────────────────────────────────────────

struct Converter<'a> {
    doc: &'a Document,
    opts: &'a ConvertOptions,
    blocks: Vec<BlockJson>,
    assets: HashMap<String, AssetJson>,
    counter: usize,
    heading_stack: Vec<(u8, String)>,
}

impl<'a> Converter<'a> {
    fn new(doc: &'a Document, opts: &'a ConvertOptions) -> Self {
        Converter {
            doc,
            opts,
            blocks: vec![],
            assets: HashMap::new(),
            counter: 0,
            heading_stack: vec![],
        }
    }

    fn next_id(&mut self) -> String {
        self.counter += 1;
        format!("b{:04}", self.counter)
    }

    fn heading_path_push(&mut self, level: u8, text: &str) -> Vec<String> {
        while self.heading_stack.last().map(|(l, _)| *l >= level).unwrap_or(false) {
            self.heading_stack.pop();
        }
        self.heading_stack.push((level, text.to_string()));
        self.heading_stack.iter().map(|(_, t)| t.clone()).collect()
    }

    fn current_heading_path(&self) -> Vec<String> {
        self.heading_stack.iter().map(|(_, t)| t.clone()).collect()
    }

    fn process_paragraphs(&mut self, paragraphs: &[Paragraph]) {
        for para in paragraphs {
            self.process_paragraph(para);
        }
    }

    fn process_paragraph(&mut self, para: &Paragraph) {
        // 컨트롤이 없으면 텍스트/heading 블록
        if para.controls.is_empty() {
            let text = clean_text(&para.text);
            if text.is_empty() {
                return;
            }
            let style_name = self
                .doc
                .doc_info
                .styles
                .get(para.style_id as usize)
                .map(|s| s.local_name.as_str())
                .unwrap_or("");
            let level = heading_level(style_name, &self.opts.heading_style_map);
            let id = self.next_id();
            if let Some(lvl) = level {
                let heading_path = self.heading_path_push(lvl, &text);
                self.blocks.push(BlockJson::Heading {
                    id,
                    level: lvl,
                    text,
                    page: None,
                    heading_path,
                });
            } else {
                let heading_path = self.current_heading_path();
                self.blocks.push(BlockJson::Paragraph {
                    id,
                    text,
                    page: None,
                    heading_path,
                });
            }
            return;
        }

        // 컨트롤 처리
        for ctrl in &para.controls {
            match ctrl {
                Control::Table(table) => {
                    self.process_table(table);
                }
                Control::Picture(pic) => {
                    self.process_picture(pic);
                }
                Control::Header(hdr) => {
                    let text = self.paras_to_text(&hdr.paragraphs);
                    if !text.is_empty() {
                        let id = self.next_id();
                        self.blocks.push(BlockJson::Header { id, text, page: None });
                    }
                }
                Control::Footer(ftr) => {
                    let text = self.paras_to_text(&ftr.paragraphs);
                    if !text.is_empty() {
                        let id = self.next_id();
                        self.blocks.push(BlockJson::Footer { id, text, page: None });
                    }
                }
                Control::Footnote(fn_) => {
                    let text = self.paras_to_text(&fn_.paragraphs);
                    if !text.is_empty() {
                        let id = self.next_id();
                        self.blocks
                            .push(BlockJson::Footnote { id, text, ref_block_id: None, page: None });
                    }
                }
                Control::Endnote(en) => {
                    let text = self.paras_to_text(&en.paragraphs);
                    if !text.is_empty() {
                        let id = self.next_id();
                        self.blocks
                            .push(BlockJson::Footnote { id, text, ref_block_id: None, page: None });
                    }
                }
                // 나머지 컨트롤은 텍스트 없으면 무시, 있으면 paragraph로
                _ => {
                    let text = clean_text(&para.text);
                    if !text.is_empty() {
                        let id = self.next_id();
                        let heading_path = self.current_heading_path();
                        self.blocks.push(BlockJson::Paragraph { id, text, page: None, heading_path });
                    }
                }
            }
        }
    }

    fn process_table(&mut self, table: &rhwp::model::table::Table) {
        let row_count = table.row_count as usize;
        let col_count = table.col_count as usize;

        // 2D 그리드 구축: cell_grid가 비어있으면 fallback
        let mut grid: Vec<Vec<String>> = vec![vec![String::new(); col_count]; row_count];
        let mut merged_cells: Vec<MergedCell> = vec![];

        if !table.cell_grid.is_empty() {
            // cell_grid[row * col_count + col] = Some(cell_idx)
            for (r, row_out) in grid.iter_mut().enumerate() {
                for (c, cell_out) in row_out.iter_mut().enumerate() {
                    let grid_idx = r * col_count + c;
                    if let Some(Some(cell_idx)) = table.cell_grid.get(grid_idx) {
                        if let Some(cell) = table.cells.get(*cell_idx) {
                            // 앵커 셀(col==c && row==r)만 값 기입 (D25)
                            if cell.col as usize == c && cell.row as usize == r {
                                *cell_out = cell_text(&cell.paragraphs);
                                if cell.col_span > 1 || cell.row_span > 1 {
                                    merged_cells.push(MergedCell {
                                        r,
                                        c,
                                        rowspan: cell.row_span,
                                        colspan: cell.col_span,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        } else {
            // fallback: cells 순서대로 row-major 배치
            for cell in &table.cells {
                let r = cell.row as usize;
                let c = cell.col as usize;
                if r < row_count && c < col_count {
                    grid[r][c] = cell_text(&cell.paragraphs);
                    if cell.col_span > 1 || cell.row_span > 1 {
                        merged_cells.push(MergedCell {
                            r,
                            c,
                            rowspan: cell.row_span,
                            colspan: cell.col_span,
                        });
                    }
                }
            }
        }

        if grid.is_empty() {
            return;
        }

        let headers: Vec<String> = grid[0].clone();
        let rows: Vec<HashMap<String, String>> = grid[1..]
            .iter()
            .map(|row| {
                headers
                    .iter()
                    .enumerate()
                    .map(|(i, k)| {
                        let key = if k.is_empty() { format!("col_{}", i) } else { k.clone() };
                        let val = row.get(i).cloned().unwrap_or_default();
                        (key, val)
                    })
                    .collect()
            })
            .collect();

        let markdown = build_markdown(&grid);
        let id = self.next_id();
        let heading_path = self.current_heading_path();

        // 캡션 처리 (D29)
        if let Some(cap) = &table.caption {
            let cap_text = self.paras_to_text(&cap.paragraphs);
            if !cap_text.is_empty() {
                let cap_id = self.next_id();
                let cap_heading_path = heading_path.clone();
                self.blocks.push(BlockJson::Caption {
                    id: cap_id,
                    text: cap_text,
                    ref_block_id: id.clone(),
                    page: None,
                    heading_path: cap_heading_path,
                });
            }
        }

        self.blocks.push(BlockJson::Table {
            id,
            page: None,
            heading_path,
            markdown,
            headers,
            rows,
            merged_cells,
        });
    }

    fn process_picture(&mut self, pic: &rhwp::model::image::Picture) {
        let bin_data_id = pic.image_attr.bin_data_id;
        let asset_ref = format!("img_{}", bin_data_id);
        let width_mm = hwp_unit_to_mm(pic.common.width);
        let height_mm = hwp_unit_to_mm(pic.common.height);

        // 이미지 데이터 추출 및 asset 등록
        if !self.assets.contains_key(&asset_ref) {
            if let Some(content) = self.find_bin_content(bin_data_id) {
                let mime = mime_from_ext(&content.extension).to_string();
                let sha256 = sha256_hex(&content.data);
                let byte_size = content.data.len();
                let data = content.data.clone();
                let ext = content.extension.clone();

                let asset = match self.opts.image_mode {
                    ImageMode::Extract => {
                        let rel_path = self.save_image_file(&asset_ref, &ext, &data);
                        AssetJson {
                            mime,
                            path: Some(rel_path),
                            data_base64: None,
                            sha256,
                            byte_size,
                        }
                    }
                    ImageMode::Inline => AssetJson {
                        mime,
                        path: None,
                        data_base64: Some(BASE64.encode(&data)),
                        sha256,
                        byte_size,
                    },
                };
                self.assets.insert(asset_ref.clone(), asset);
            }
        }

        let id = self.next_id();
        let heading_path = self.current_heading_path();

        // 캡션 처리 (D29)
        if let Some(cap) = &pic.caption {
            let cap_text = self.paras_to_text(&cap.paragraphs);
            if !cap_text.is_empty() {
                let cap_id = self.next_id();
                let cap_hp = heading_path.clone();
                self.blocks.push(BlockJson::Caption {
                    id: cap_id,
                    text: cap_text,
                    ref_block_id: id.clone(),
                    page: None,
                    heading_path: cap_hp,
                });
            }
        }

        self.blocks.push(BlockJson::Image {
            id,
            page: None,
            heading_path,
            asset_ref,
            alt: None,
            width_mm,
            height_mm,
            near_text_before: None,
            near_text_after: None,
        });
    }

    fn find_bin_content(&self, bin_data_id: u16) -> Option<&rhwp::model::bin_data::BinDataContent> {
        if bin_data_id == 0 {
            return None;
        }
        let bin_data = self.doc.doc_info.bin_data_list.get(bin_data_id as usize - 1)?;
        self.doc
            .bin_data_content
            .iter()
            .find(|c| c.id == bin_data.storage_id)
    }

    fn save_image_file(&self, asset_ref: &str, ext: &str, data: &[u8]) -> String {
        let filename = format!("{}.{}", asset_ref, ext);
        if let Some(dir) = &self.opts.image_dir {
            let _ = std::fs::create_dir_all(dir);
            let path = dir.join(&filename);
            let _ = std::fs::write(&path, data);
            path.to_string_lossy().to_string()
        } else {
            // image_dir이 None이면 런타임에 서비스 레이어에서 설정되므로 상대 경로 반환
            filename
        }
    }

    fn paras_to_text(&self, paragraphs: &[Paragraph]) -> String {
        paragraphs
            .iter()
            .map(|p| clean_text(&p.text))
            .filter(|t| !t.is_empty())
            .collect::<Vec<_>>()
            .join("\n")
    }
}

/// 2D 셀 그리드를 Markdown 표로 직렬화
fn build_markdown(grid: &[Vec<String>]) -> String {
    if grid.is_empty() {
        return String::new();
    }
    let col_count = grid[0].len();
    let mut out = String::new();

    let escape = |s: &str| s.replace('|', "\\|").replace('\n', "<br>");

    // 헤더 행
    out.push('|');
    for cell in &grid[0] {
        out.push(' ');
        out.push_str(&escape(cell));
        out.push_str(" |");
    }
    out.push('\n');

    // 구분선
    out.push('|');
    for _ in 0..col_count {
        out.push_str("-----|");
    }
    out.push('\n');

    // 데이터 행
    for row in &grid[1..] {
        out.push('|');
        for i in 0..col_count {
            let cell = row.get(i).map(|s| s.as_str()).unwrap_or("");
            out.push(' ');
            out.push_str(&escape(cell));
            out.push_str(" |");
        }
        out.push('\n');
    }

    out.trim_end_matches('\n').to_string()
}

/// Image 블록의 near_text_before/after를 사후 채움
fn fill_near_text(blocks: &mut [BlockJson]) {
    // 먼저 각 인덱스의 adjacent text를 미리 추출 (borrow checker 우회)
    let texts: Vec<Option<String>> = blocks
        .iter()
        .map(|b| b.as_adjacent_text().map(String::from))
        .collect();

    let total = texts.len();
    for (i, block) in blocks.iter_mut().enumerate() {
        if let BlockJson::Image { near_text_before, near_text_after, .. } = block {
            *near_text_before = (0..i).rev().find_map(|j| texts[j].clone());
            *near_text_after = (i + 1..total).find_map(|j| texts[j].clone());
        }
    }
}

// ─────────────────────────────────────────────────────────────

/// Document IR → DocumentJson 변환 진입점
pub fn convert(doc: &Document, opts: &ConvertOptions) -> DocumentJson {
    let mut c = Converter::new(doc, opts);

    for section in &doc.sections {
        c.process_paragraphs(&section.paragraphs);
    }

    fill_near_text(&mut c.blocks);

    DocumentJson {
        schema_version: "1.0.0".to_string(),
        source: SourceJson {
            filename: opts.source_filename.clone(),
            format: opts.source_format.clone(),
            sha256: opts.source_sha256.clone(),
            section_count: doc.sections.len(),
            extracted_at: current_iso8601(),
        },
        blocks: c.blocks,
        assets: c.assets,
    }
}

// ─────────────────────────────────────────────────────────────
// 파일 sha256 계산 헬퍼 (서비스 레이어에서 사용)
pub fn sha256_file(data: &[u8]) -> String {
    sha256_hex(data)
}
