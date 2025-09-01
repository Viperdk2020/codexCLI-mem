pub struct Redaction {
    pub masked: String,
    pub issues: Vec<String>,
    pub blocked: bool,
}

pub fn redact_candidate(_s: &str) -> Redaction {
    todo!()
}
