pub struct Redaction {
    pub masked: String,
    pub issues: Vec<String>,
    pub blocked: bool,
}

pub fn redact_candidate(s: &str) -> Redaction {
    use regex::Regex;

    // Issues discovered while scanning the input and byte ranges to mask.
    let mut issues = Vec::new();
    let mut spans: Vec<(usize, usize)> = Vec::new();

    fn push_span(
        spans: &mut Vec<(usize, usize)>,
        issues: &mut Vec<String>,
        range: (usize, usize),
        issue: &str,
    ) {
        if spans.iter().any(|(s, e)| range.0 >= *s && range.1 <= *e) {
            return;
        }
        spans.push(range);
        issues.push(issue.to_string());
    }

    // API keys, tokens or secrets of the form NAME=VALUE where VALUE is long.
    let api_re =
        Regex::new(r"(?i)(api[_-]?key|token|secret|password)[\s:=]+([A-Za-z0-9_\-]{16,})").unwrap();
    for caps in api_re.captures_iter(s) {
        if let Some(mat) = caps.get(2) {
            push_span(&mut spans, &mut issues, (mat.start(), mat.end()), "possible API key");
        }
    }

    // SSH public keys or PEM encoded private keys.
    let ssh_re = Regex::new(r"ssh-(rsa|ed25519) [A-Za-z0-9+/=]{20,}").unwrap();
    for mat in ssh_re.find_iter(s) {
        push_span(&mut spans, &mut issues, (mat.start(), mat.end()), "possible SSH key");
    }

    let pem_re =
        Regex::new(r"-----BEGIN [A-Z ]*PRIVATE KEY-----[\s\S]+?-----END [A-Z ]*PRIVATE KEY-----")
            .unwrap();
    for mat in pem_re.find_iter(s) {
        push_span(&mut spans, &mut issues, (mat.start(), mat.end()), "possible private key");
    }

    // High entropy strings: long base64/hex-like tokens.
    let ent_re = Regex::new(r"[A-Za-z0-9+/=_-]{20,}").unwrap();
    for mat in ent_re.find_iter(s) {
        let token = mat.as_str();
        if spans
            .iter()
            .any(|(start, end)| mat.start() < *end && mat.end() > *start)
        {
            continue;
        }
        if shannon_entropy(token) >= 4.5 {
            push_span(&mut spans, &mut issues, (mat.start(), mat.end()), "high-entropy string");
        }
    }

    spans.sort_by_key(|r| r.0);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in spans.into_iter() {
        if let Some(last) = merged.last_mut() && start <= last.1 {
            last.1 = last.1.max(end);
            continue;
        }
        merged.push((start, end));
    }

    // Build the masked string.
    let mut masked = String::new();
    let mut last = 0usize;
    for (start, end) in merged {
        if start > last {
            masked.push_str(&s[last..start]);
        }
        masked.push_str("[REDACTED]");
        last = end;
    }
    if last < s.len() {
        masked.push_str(&s[last..]);
    }

    let blocked = !issues.is_empty();
    Redaction {
        masked,
        issues,
        blocked,
    }
}

fn shannon_entropy(s: &str) -> f64 {
    let mut freq = [0u32; 256];
    let mut len = 0usize;
    for b in s.bytes() {
        freq[b as usize] += 1;
        len += 1;
    }
    let mut ent = 0f64;
    for &count in &freq {
        if count > 0 {
            let p = count as f64 / len as f64;
            ent -= p * p.log2();
        }
    }
    ent
}
