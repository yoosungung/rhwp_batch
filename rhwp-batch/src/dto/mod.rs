use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct DocumentJson {
    pub schema_version: String,
    pub format: String,
    pub metadata: MetadataJson,
    pub sections: Vec<SectionJson>,
    #[serde(default, skip_serializing_if = "std::collections::HashMap::is_empty")]
    pub assets: std::collections::HashMap<String, AssetJson>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct MetadataJson {
    pub version: String,
    pub section_count: usize,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SectionJson {
    pub paragraphs: Vec<ParagraphJson>,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ParagraphJson {
    Text {
        #[serde(default, skip_serializing_if = "String::is_empty")]
        style: String,
        text: String,
    },
    Table {
        rows: Vec<Vec<CellJson>>,
    },
    Image {
        #[serde(rename = "ref")]
        asset_ref: String,
        width_mm: f64,
        height_mm: f64,
    },
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CellJson {
    pub text: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct AssetJson {
    pub mime: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data_base64: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}
