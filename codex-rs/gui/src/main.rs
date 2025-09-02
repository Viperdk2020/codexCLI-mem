use clap::Parser;
use eframe::egui;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tracing_subscriber::EnvFilter;

use chrono::Utc;
use codex_memory::factory;
use codex_memory::recall::{recall, RecallContext};
use codex_memory::types::Counters;
use codex_memory::types::Kind;
use codex_memory::types::MemoryItem;
use codex_memory::types::RelevanceHints;
use codex_memory::types::Scope;
use codex_memory::types::Status;
use uuid::Uuid;

#[derive(clap::ValueEnum, Clone, Debug)]
enum MemoryToggle {
    On,
    Off,
    Auto,
}

#[derive(clap::ValueEnum, Clone, Debug)]
enum RendererToggle {
    Auto,
    Wgpu,
    Glow,
}

#[derive(Parser, Debug, Clone)]
#[command(author, version, about = "Codex GUI (egui MVP)")]
struct Args {
    /// Working directory (repo root autodetected if omitted)
    #[arg(long = "cwd")]
    cwd: Option<PathBuf>,

    /// Control per-repo memory logging for this run (default auto)
    #[arg(long = "memory", value_enum, default_value_t = MemoryToggle::Auto)]
    memory: MemoryToggle,

    /// Select renderer backend (wgpu or glow)
    #[arg(long = "renderer", value_enum, default_value_t = RendererToggle::Auto)]
    renderer: RendererToggle,

    /// Run without opening a window; perform memory ops and exit
    #[arg(long = "headless", default_value_t = false)]
    headless: bool,

    /// Headless: prompt text used for save/recall
    #[arg(long = "prompt")]
    prompt: Option<String>,

    /// Headless: save --prompt to repo memory
    #[arg(long = "save", default_value_t = false)]
    headless_save: bool,

    /// Headless: recall relevant items for --prompt
    #[arg(long = "recall", default_value_t = false)]
    headless_recall: bool,
}

fn main() {
    // Logging
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        // Tame noisy clipboard/X11/Wayland and shader logs in headless or minimal sessions.
        // Users can override with RUST_LOG.
        EnvFilter::new("info,arboard=off,smithay_clipboard=off,sctk_adwaita=off,naga=warn")
    });
    // Bridge `log` records (from deps like arboard/winit) into tracing so filters apply.
    let _ = tracing_log::LogTracer::init();
    let _ = tracing_subscriber::fmt()
        .with_env_filter(env_filter)
        .try_init();

    let args = Args::parse();
    if is_wsl() {
        // Harden defaults for WSLg to avoid Wayland/XDG portal stalls.
        unsafe { std::env::set_var("SCTK_ADWAITA_NO_PORTAL", "1") };
        // Prefer X11 path which is generally more reliable in WSLg today.
        unsafe { std::env::set_var("WINIT_UNIX_BACKEND", "x11") };
        // If user chose WGPU, prefer GL over Vulkan to avoid llvmpipe/Vulkan path.
        if matches!(args.renderer, RendererToggle::Auto | RendererToggle::Wgpu) {
            unsafe { std::env::set_var("WGPU_BACKEND", "gl") };
        }
    }
    eprintln!(
        "DISPLAY={:?} WAYLAND_DISPLAY={:?} WSL_DETECTED={}",
        std::env::var("DISPLAY").ok(),
        std::env::var("WAYLAND_DISPLAY").ok(),
        is_wsl()
    );

    if args.headless {
        if let Err(e) = run_headless(&args) {
            eprintln!("Headless error: {e}");
            std::process::exit(1);
        }
        return;
    }
    let (tx, rx) = unbounded_channel::<FrontendMsg>();
    std::thread::spawn(move || backend_thread(rx));

    // Move owned copies into the closure to satisfy 'static bounds.
    let args_owned = args.clone();
    let tx_owned = tx.clone();
    let run_with = move |renderer: eframe::Renderer| {
        let native_options = eframe::NativeOptions { renderer, ..Default::default() };
        let args_for_app = args_owned.clone();
        let tx_for_app = tx_owned.clone();
        eframe::run_native(
            "Codex GUI",
            native_options,
            Box::new(move |cc| Ok(Box::new(CodexGui::new(cc, args_for_app.clone(), tx_for_app.clone())))),
        )
    };

    let res = match args.renderer {
        RendererToggle::Wgpu => run_with(eframe::Renderer::Wgpu),
        RendererToggle::Glow => {
            match run_with(eframe::Renderer::Glow) {
                Ok(()) => Ok(()),
                Err(e) => {
                    tracing::warn!("Glow init failed: {}", e);
                    if is_wsl() {
                        unsafe { std::env::set_var("WGPU_BACKEND", "gl") };
                        tracing::info!("Retrying with WGPU (GL backend) due to Glow failure on WSL");
                        run_with(eframe::Renderer::Wgpu)
                    } else {
                        Err(e)
                    }
                }
            }
        },
        RendererToggle::Auto => {
            if is_wsl() {
                tracing::info!("WSL detected: preferring Glow backend in Auto mode");
                match run_with(eframe::Renderer::Glow) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        tracing::warn!("Glow init failed: {}", e);
                        unsafe { std::env::set_var("WGPU_BACKEND", "gl") };
                        tracing::info!("Retrying with WGPU (GL backend) due to Glow failure on WSL");
                        run_with(eframe::Renderer::Wgpu)
                    }
                }
            } else {
                match run_with(eframe::Renderer::Wgpu) {
                    Ok(()) => Ok(()),
                    Err(e) => {
                        tracing::warn!("WGPU backend failed: {} — retrying with Glow", e);
                        run_with(eframe::Renderer::Glow)
                    }
                }
            }
        }
    };
    if let Err(e) = res {
        eprintln!("Failed to start Codex GUI: {e}");
    }
}

fn is_wsl() -> bool {
    // Detect WSL by checking /proc/version and environment hints.
    let ver = std::fs::read_to_string("/proc/version").unwrap_or_default();
    ver.to_ascii_lowercase().contains("microsoft")
        || std::env::var("WSL_DISTRO_NAME").is_ok()
        || std::env::var("WSL_INTEROP").is_ok()
}

fn run_headless(args: &Args) -> anyhow::Result<()> {
    let repo_root = args
        .cwd
        .clone()
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));
    let store = factory::open_repo_store(&repo_root, None)?;

    if args.headless_save {
        let prompt = args
            .prompt
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--prompt required with --save"))?;
        let now = Utc::now().to_rfc3339();
        let item = MemoryItem {
            id: Uuid::new_v4().to_string(),
            created_at: now.clone(),
            updated_at: now,
            schema_version: 1,
            source: "codex-gui(headless)".into(),
            scope: Scope::Repo,
            status: Status::Active,
            kind: Kind::Note,
            content: prompt.clone(),
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
        println!("saved");
    }

    if args.headless_recall {
        let prompt = args
            .prompt
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("--prompt required with --recall"))?;
        let ctx = RecallContext {
            repo_root: Some(repo_root.clone()),
            dir: None,
            current_file: None,
            crate_name: None,
            language: None,
            command: None,
            now_rfc3339: Utc::now().to_rfc3339(),
            item_cap: 0,
            token_cap: 0,
        };
        let items = recall(store.as_ref(), prompt, &ctx)?;
        let texts: Vec<String> = items.into_iter().map(|i| i.content).collect();
        println!("{}", serde_json::to_string(&texts)?);
    }

    if !args.headless_save && !args.headless_recall {
        // Default headless action: list active items
        let items = store.list(Some(Scope::Repo), Some(Status::Active))?;
        for i in items {
            println!("{}", i.content);
        }
    }

    Ok(())
}

// Placeholder backend thread – will integrate codex-core events later.
fn backend_thread(_rx: UnboundedReceiver<FrontendMsg>) {
    // For MVP skeleton we do nothing here.
}

#[derive(Clone, Debug)]
enum FrontendMsg {
    SendPrompt(()),
}

struct CodexGui {
    args: Args,
    to_backend: UnboundedSender<FrontendMsg>,
    // UI state
    prompt: String,
    transcript: Vec<String>,
    memory_items: Vec<String>,
    repo_root: PathBuf,
    recall_items: Vec<String>,
}

impl CodexGui {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        args: Args,
        to_backend: UnboundedSender<FrontendMsg>,
    ) -> Self {
        let repo_root = args
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        let mut this = Self {
            args,
            to_backend,
            prompt: String::new(),
            transcript: Vec::new(),
            memory_items: Vec::new(),
            repo_root,
            recall_items: Vec::new(),
        };
        this.refresh_memory_safely();
        this
    }

    fn refresh_memory_safely(&mut self) {
        match factory::open_repo_store(&self.repo_root, None) {
            Ok(store) => match store.list(Some(Scope::Repo), Some(Status::Active)) {
                Ok(items) => {
                    self.memory_items = items.into_iter().map(|i| i.content).collect();
                }
                Err(e) => {
                    tracing::warn!("failed to list memory items: {}", e);
                }
            },
            Err(e) => tracing::warn!("failed to open memory store: {}", e),
        }
    }

    fn add_prompt_to_memory_safely(&mut self) {
        if self.prompt.trim().is_empty() {
            return;
        }
        match factory::open_repo_store(&self.repo_root, None) {
            Ok(store) => {
                let now = Utc::now().to_rfc3339();
                let item = MemoryItem {
                    id: Uuid::new_v4().to_string(),
                    created_at: now.clone(),
                    updated_at: now,
                    schema_version: 1,
                    source: "codex-gui".into(),
                    scope: Scope::Repo,
                    status: Status::Active,
                    kind: Kind::Note,
                    content: self.prompt.clone(),
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
                if let Err(e) = store.add(item) {
                    tracing::warn!("failed to add memory item: {}", e);
                }
                self.refresh_memory_safely();
            }
            Err(e) => tracing::warn!("failed to open memory store: {}", e),
        }
    }

    fn perform_recall_safely(&mut self, query: &str) {
        if query.trim().is_empty() {
            self.recall_items.clear();
            return;
        }
        match factory::open_repo_store(&self.repo_root, None) {
            Ok(store) => {
                let ctx = RecallContext {
                    repo_root: Some(self.repo_root.clone()),
                    dir: None,
                    current_file: None,
                    crate_name: None,
                    language: None,
                    command: None,
                    now_rfc3339: Utc::now().to_rfc3339(),
                    item_cap: 0,
                    token_cap: 0,
                };
                match recall(store.as_ref(), query, &ctx) {
                    Ok(items) => {
                        self.recall_items = items.into_iter().map(|i| i.content).collect();
                    }
                    Err(e) => tracing::warn!("failed to recall memory: {}", e),
                }
            }
            Err(e) => tracing::warn!("failed to open memory store: {}", e),
        }
    }
}

impl eframe::App for CodexGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Codex GUI — MVP");
                ui.separator();
                ui.label(format!(
                    "cwd: {}",
                    self.args
                        .cwd
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "auto".into())
                ));
                ui.separator();
                ui.label(format!("memory: {:?}", self.args.memory));
            });
        });

        egui::TopBottomPanel::bottom("composer").show(ctx, |ui| {
            ui.separator();
            ui.label("Ask Codex:");
            let r = egui::TextEdit::multiline(&mut self.prompt)
                .desired_rows(3)
                .hint_text("Type a prompt…")
                .lock_focus(true)
                .show(ui);
            if r.response.lost_focus()
                && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.shift_only())
            {
                self.to_backend
                    .send(FrontendMsg::SendPrompt(()))
                    .ok();
                self.transcript.push(format!("You: {}", self.prompt));
                let q = self.prompt.clone();
                self.perform_recall_safely(&q);
                self.prompt.clear();
            }
            ui.horizontal(|ui| {
                if ui.button("Send (Shift+Enter)").clicked() {
                    self.to_backend
                        .send(FrontendMsg::SendPrompt(()))
                        .ok();
                    self.transcript.push(format!("You: {}", self.prompt));
                    let q = self.prompt.clone();
                    self.perform_recall_safely(&q);
                    self.prompt.clear();
                }
                if ui.button("Save to Memory").clicked() {
                    self.add_prompt_to_memory_safely();
                }
                if ui.button("Recall").clicked() {
                    let q = self.prompt.clone();
                    self.perform_recall_safely(&q);
                }
                if ui.button("Refresh Memory").clicked() {
                    self.refresh_memory_safely();
                }
                if ui.button("Clear").clicked() {
                    self.prompt.clear();
                }
            });
        });

        egui::SidePanel::right("memory_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.heading("Project Memory");
                if self.memory_items.is_empty() {
                    ui.label("No durable items yet.");
                }
                for item in &self.memory_items {
                    ui.label(item);
                }
            });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.columns(2, |cols| {
                cols[0].heading("Transcript");
                egui::ScrollArea::vertical().show(&mut cols[0], |ui| {
                    for line in &self.transcript {
                        ui.label(line);
                        ui.separator();
                    }
                });

                cols[1].heading("Relevant Memory (Recall)");
                egui::ScrollArea::vertical().show(&mut cols[1], |ui| {
                    if self.recall_items.is_empty() {
                        ui.label("No relevant items yet.");
                    }
                    for item in &self.recall_items {
                        ui.label(item);
                        ui.separator();
                    }
                });
            });
        });
    }
}
