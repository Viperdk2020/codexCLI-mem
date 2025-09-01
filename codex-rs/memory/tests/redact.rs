
use codex_memory::factory::Backend;
use codex_memory::redact::redact_candidate;

fn backends() -> Vec<Backend> {
    #[cfg(feature = "sqlite")]
    {
        vec![Backend::Jsonl, Backend::Sqlite]
    }
    #[cfg(not(feature = "sqlite"))]
    {
        vec![Backend::Jsonl]
    }
}

#[test]
fn redact_unimplemented_panics() {
    for _be in backends() {
        let res = std::panic::catch_unwind(|| redact_candidate("secret"));
        assert!(res.is_err());
    }
  
use codex_memory::redact::redact_candidate;

#[test]
fn api_key_detection() {
    let input = "Here is API_KEY=ABCD1234EFGH5678IJKL9012";
    let result = redact_candidate(input);
    assert!(result.blocked);
    assert!(result.issues.iter().any(|i| i.contains("API key")));
    assert_eq!(result.masked, "Here is API_KEY=[REDACTED]");
}

#[test]
fn ssh_key_detection() {
    let input = "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIBS8up32jhRz25k4b1qm0Nn1ta1Vx";
    let result = redact_candidate(input);
    assert!(result.blocked);
    assert!(result.issues.iter().any(|i| i.contains("SSH key")));
    assert_eq!(result.masked, "[REDACTED]");
}

#[test]
fn high_entropy_detection() {
    let input = "token: XyZ0123456789+/ABCdefghIJKLmnoPQRstuVWxyz0123";
    let result = redact_candidate(input);
    assert!(result.blocked);
    assert!(result.issues.iter().any(|i| i.contains("high-entropy")));
    assert_eq!(result.masked, "token: [REDACTED]");
}

#[test]
fn no_detection() {
    let input = "ordinary text";
    let result = redact_candidate(input);
    assert!(!result.blocked);
    assert!(result.issues.is_empty());
    assert_eq!(result.masked, input);

}
