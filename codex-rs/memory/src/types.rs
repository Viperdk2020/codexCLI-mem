#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Scope {
    Global,
    Repo,
    Dir,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Kind {
    Pref,
    Fact,
    Profile,
    Instruction,
    Note,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub enum Status {
    Active,
    Archived,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct RelevanceHints {
    pub files: Vec<String>,
    pub crates: Vec<String>,
    pub languages: Vec<String>,
    pub commands: Vec<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Counters {
    pub seen_count: u32,
    pub used_count: u32,
    pub last_used_at: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Expiry {
    pub ttl_secs: Option<u64>,
    pub review_after: Option<String>,
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MemoryItem {
    pub id: String,
    pub created_at: String,
    pub updated_at: String,
    pub schema_version: u16,
    pub source: String,
    pub scope: Scope,
    pub status: Status,
    pub kind: Kind,
    pub content: String,
    pub tags: Vec<String>,
    pub relevance_hints: RelevanceHints,
    pub counters: Counters,
    pub expiry: Option<Expiry>,
}
