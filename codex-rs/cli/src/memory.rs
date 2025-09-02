use chrono::Utc;
use clap::Parser;
use codex_memory::factory;
use codex_memory::types::Counters;
use codex_memory::types::Kind;
use codex_memory::types::MemoryItem;
use codex_memory::types::RelevanceHints;
use codex_memory::types::Scope;
use codex_memory::types::Status;
use std::path::PathBuf;
use uuid::Uuid;

/// CLI for memory management commands.
#[derive(Debug, Parser)]
pub struct MemoryCli {
    #[command(subcommand)]
    pub cmd: MemoryCommand,
}

/// Memory subcommands.
#[derive(Debug, clap::Subcommand)]
pub enum MemoryCommand {
    /// Add a new memory item with given content.
    Add { content: String },
    /// List memory items.
    List,
    /// Edit an existing memory item.
    Edit { id: String, content: String },
    /// Remove a memory item by id.
    Rm { id: String },
    /// Archive a memory item.
    Archive { id: String },
    /// Unarchive a memory item.
    Unarchive { id: String },
    /// Export memory items to stdout.
    Export,
    /// Import memory items from stdin.
    Import,
    /// Migrate a JSONL file to a SQLite database.
    Migrate {
        /// Path to the source JSONL file
        #[arg(long)]
        jsonl: PathBuf,
        /// Path to the destination SQLite database file
        #[arg(long)]
        sqlite: PathBuf,
    },
    /// Compact a JSONL file by removing duplicate entries.
    Compact {
        /// Input JSONL file to compact
        #[arg(long)]
        input: PathBuf,
        /// Output JSONL file to write results
        #[arg(long)]
        output: PathBuf,
    },
    /// Show basic statistics about stored memories.
    Stats,
    /// Recall memories for a given prompt.
    Recall {
        #[arg(long = "for")]
        query: String,
    },
}

/// Execute the memory command.
pub fn run(cli: MemoryCli) -> anyhow::Result<()> {
    match cli.cmd {
        MemoryCommand::Migrate { jsonl, sqlite } => {
            let n = codex_memory::migrate::migrate_jsonl_to_sqlite(&jsonl, &sqlite)?;
            println!("Migrated {n} entries");
        }
        cmd => {
            let repo_root = std::env::current_dir()?;
            let store = factory::open_repo_store(&repo_root, None)?;
            match cmd {
                MemoryCommand::Add { content } => {
                    let now = Utc::now().to_rfc3339();
                    let item = MemoryItem {
                        id: Uuid::new_v4().to_string(),
                        created_at: now.clone(),
                        updated_at: now,
                        schema_version: 1,
                        source: "codex-cli".into(),
                        scope: Scope::Repo,
                        status: Status::Active,
                        kind: Kind::Note,
                        content,
                        tags: Vec::new(),
                        relevance_hints: RelevanceHints {
                            files: Vec::new(),
                            crates: Vec::new(),
                            languages: Vec::new(),
                            commands: Vec::new(),
                        },
                        counters: Counters {
                            seen_count: 0,
                            used_count: 0,
                            last_used_at: None,
                        },
                        expiry: None,
                    };
                    store.add(item)?;
                }
                MemoryCommand::List => {
                    for item in store.list(None, None)? {
                        println!("{}", item.content);
                    }
                }
                MemoryCommand::Edit { id, content } => {
                    if let Some(mut item) = store.get(&id)? {
                        item.content = content;
                        item.updated_at = Utc::now().to_rfc3339();
                        store.update(&item)?;
                    } else {
                        anyhow::bail!("memory id not found: {id}");
                    }
                }
                MemoryCommand::Rm { id } => {
                    store.delete(&id)?;
                }
                MemoryCommand::Archive { id } => {
                    store.archive(&id, true)?;
                }
                MemoryCommand::Unarchive { id } => {
                    store.archive(&id, false)?;
                }
                MemoryCommand::Export => {
                    let mut out = std::io::stdout();
                    store.export(&mut out)?;
                }
                MemoryCommand::Import => {
                    let mut input = std::io::stdin();
                    let n = store.import(&mut input)?;
                    println!("Imported {n} items");
                }
                MemoryCommand::Stats => {
                    let stats = store.stats()?;
                    println!("{stats}");
                }
                MemoryCommand::Recall { query } => {
                    let ctx = codex_memory::recall::RecallContext {
                        repo_root: Some(repo_root),
                        dir: None,
                        current_file: None,
                        crate_name: None,
                        language: None,
                        command: None,
                        now_rfc3339: Utc::now().to_rfc3339(),
                        item_cap: 8,
                        token_cap: 300,
                    };
                    let items = codex_memory::recall::recall(store.as_ref(), &query, &ctx)?;
                    println!("{}", serde_json::to_string(&items)?);
                }
                MemoryCommand::Compact { input, output } => {
                    let (read, written) = codex_memory::migrate::compact_jsonl(&input, &output)?;
                    println!("Read {read} entries, wrote {written} entries");
                }
                MemoryCommand::Migrate { .. } => unreachable!(),
            }
        }
    }
    Ok(())
}
