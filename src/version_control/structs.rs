use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct UpdateManifest {
    #[serde(flatten)]
    pub list: BTreeMap<String, Vec<Update>>,
}

#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Update {
    pub chainlink: bool,
    pub checksum: String,
    pub version: String,
    pub changelog: String,
    pub file_size: Option<String>,
}
