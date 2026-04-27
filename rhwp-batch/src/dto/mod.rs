use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// RAG ingestion을 위한 최상위 문서 DTO.
/// 스키마 정본: ROADMAP.md §1.3 / D2.
#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentJson {
    pub schema_version: String,
    pub source: SourceJson,
    pub blocks: Vec<BlockJson>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub assets: HashMap<String, AssetJson>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SourceJson {
    pub filename: String,
    /// "hwp" | "hwpx"
    pub format: String,
    pub sha256: String,
    pub section_count: usize,
    pub extracted_at: String,
}

/// RAG 청크 후보 블록. `type` 필드로 variant 구분 (serde tag).
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum BlockJson {
    Heading {
        id: String,
        /// 1~6 정규화 (D23)
        level: u8,
        text: String,
        page: Option<u32>,
        heading_path: Vec<String>,
    },
    Paragraph {
        id: String,
        text: String,
        page: Option<u32>,
        heading_path: Vec<String>,
    },
    ListItem {
        id: String,
        text: String,
        page: Option<u32>,
        heading_path: Vec<String>,
        level: u8,
    },
    Table {
        id: String,
        page: Option<u32>,
        heading_path: Vec<String>,
        /// 임베딩용 Markdown (D26: 멀티라인 셀 → <br>)
        markdown: String,
        /// 헤더 키 목록 (row 0)
        headers: Vec<String>,
        /// 구조화 rows (D25: 병합셀 앵커에만 값, 나머지 빈 문자열)
        rows: Vec<HashMap<String, String>>,
        /// 병합셀 메타 (D25)
        merged_cells: Vec<MergedCell>,
    },
    Image {
        id: String,
        page: Option<u32>,
        heading_path: Vec<String>,
        asset_ref: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        alt: Option<String>,
        width_mm: f64,
        height_mm: f64,
        /// 직전 paragraph/heading 텍스트 (검색 회수 보강)
        #[serde(skip_serializing_if = "Option::is_none")]
        near_text_before: Option<String>,
        /// 직후 paragraph/heading 텍스트
        #[serde(skip_serializing_if = "Option::is_none")]
        near_text_after: Option<String>,
    },
    /// 머리말 (D24: 항상 포함, downstream 필터링)
    Header {
        id: String,
        text: String,
        /// 모든 페이지에 적용 → None
        page: Option<u32>,
    },
    /// 꼬리말 (D24)
    Footer {
        id: String,
        text: String,
        page: Option<u32>,
    },
    Footnote {
        id: String,
        text: String,
        #[serde(skip_serializing_if = "Option::is_none")]
        ref_block_id: Option<String>,
        page: Option<u32>,
    },
    /// 표·그림 캡션 별도 블록 (D29)
    Caption {
        id: String,
        text: String,
        ref_block_id: String,
        page: Option<u32>,
        heading_path: Vec<String>,
    },
}

impl BlockJson {
    pub fn id(&self) -> &str {
        match self {
            BlockJson::Heading { id, .. }
            | BlockJson::Paragraph { id, .. }
            | BlockJson::ListItem { id, .. }
            | BlockJson::Table { id, .. }
            | BlockJson::Image { id, .. }
            | BlockJson::Header { id, .. }
            | BlockJson::Footer { id, .. }
            | BlockJson::Footnote { id, .. }
            | BlockJson::Caption { id, .. } => id,
        }
    }

    /// Paragraph/Heading 텍스트만 추출 (near_text 계산용)
    pub fn as_adjacent_text(&self) -> Option<&str> {
        match self {
            BlockJson::Paragraph { text, .. } | BlockJson::Heading { text, .. }
                if !text.is_empty() =>
            {
                Some(text)
            }
            _ => None,
        }
    }
}

/// 병합셀 메타 (D25)
#[derive(Debug, Serialize, Deserialize)]
pub struct MergedCell {
    pub r: usize,
    pub c: usize,
    pub rowspan: u16,
    pub colspan: u16,
}

/// 이미지 자산 (D4: 기본 extract)
#[derive(Debug, Serialize, Deserialize)]
pub struct AssetJson {
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    pub sha256: String,
    pub byte_size: usize,
}
