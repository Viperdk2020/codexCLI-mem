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
}
