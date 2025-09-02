use clap::Parser;
use eframe::egui;
use std::path::PathBuf;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::mpsc::unbounded_channel;
use tracing_subscriber::EnvFilter;

use chrono::Utc;
use codex_core::codex::Codex;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::config::find_codex_home;
use codex_core::protocol::EventMsg;
use codex_core::protocol::InputItem as AgentInputItem;
use codex_core::protocol::Op as AgentOp;
use codex_login::AuthManager;
use codex_memory::factory;
use codex_memory::recall::RecallContext;
use codex_memory::recall::recall;
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
    let (tx, rx_frontend) = unbounded_channel::<FrontendMsg>();
    let (tx_backend, rx_backend) = unbounded_channel::<BackendMsg>();
    std::thread::spawn(move || backend_thread(rx_frontend, tx_backend));

    // Move owned copies into the closure to satisfy 'static bounds.
    let args_owned = args.clone();
    let tx_owned = tx.clone();
    let rx_backend_cell = std::sync::Arc::new(std::sync::Mutex::new(Some(rx_backend)));
    let run_with = move |renderer: eframe::Renderer| {
        let native_options = eframe::NativeOptions {
            renderer,
            ..Default::default()
        };
        let args_for_app = args_owned.clone();
        let tx_for_app = tx_owned.clone();
        eframe::run_native(
            "Codex GUI",
            native_options,
            Box::new(move |cc| {
                let rx_for_app = rx_backend_cell
                    .lock()
                    .ok()
                    .and_then(|mut g| g.take())
                    .expect("rx_backend reused");
                Ok(Box::new(CodexGui::new(
                    cc,
                    args_for_app.clone(),
                    tx_for_app.clone(),
                    rx_for_app,
                )))
            }),
        )
    };

    let chosen = match args.renderer {
        RendererToggle::Wgpu => eframe::Renderer::Wgpu,
        RendererToggle::Glow => eframe::Renderer::Glow,
        RendererToggle::Auto => {
            if is_wsl() {
                eframe::Renderer::Glow
            } else {
                eframe::Renderer::Wgpu
            }
        }
    };
    let res = run_with(chosen);
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
            item_cap: 8,
            token_cap: 300,
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
fn backend_thread(
    rx_frontend: UnboundedReceiver<FrontendMsg>,
    tx_backend: UnboundedSender<BackendMsg>,
) {
    let rt = match tokio::runtime::Runtime::new() {
        Ok(rt) => rt,
        Err(e) => {
            let _ = tx_backend.send(BackendMsg::Error(format!("tokio runtime init failed: {e}")));
            return;
        }
    };
    rt.block_on(async move {
        let cwd = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        let overrides = ConfigOverrides {
            cwd: Some(cwd.clone()),
            ..Default::default()
        };
        let config = match Config::load_with_cli_overrides(vec![], overrides) {
            Ok(c) => c,
            Err(e) => {
                let _ = tx_backend.send(BackendMsg::Error(format!("config load failed: {e}")));
                return;
            }
        };
        let codex_home = match find_codex_home() {
            Ok(p) => p,
            Err(e) => {
                let _ = tx_backend.send(BackendMsg::Error(format!("find_codex_home failed: {e}")));
                return;
            }
        };
        let auth_manager = AuthManager::shared(codex_home, config.preferred_auth_method);
        if auth_manager.auth().is_none() {
            let _ = tx_backend.send(BackendMsg::AuthMissing);
        }
        if auth_manager.auth().is_none() {
            let _ = tx_backend.send(BackendMsg::AuthMissing);
        }
        let spawn_ok = match Codex::spawn(config.clone(), auth_manager, None).await {
            Ok(ok) => ok,
            Err(e) => {
                let _ = tx_backend.send(BackendMsg::Error(format!("codex spawn failed: {e}")));
                return;
            }
        };
        let mut rx = rx_frontend;
        loop {
            tokio::select! {
                evt = spawn_ok.codex.next_event() => {
                    match evt {
                        Ok(ev) => match ev.msg {
                            EventMsg::AgentMessage(m) => { let _ = tx_backend.send(BackendMsg::AgentText(m.message)); }
                            EventMsg::AgentMessageDelta(d) => { let _ = tx_backend.send(BackendMsg::AgentDelta(d.delta)); }
                            EventMsg::AgentReasoning(r) => { let _ = tx_backend.send(BackendMsg::Reasoning(r.text)); }
                            EventMsg::Error(err) => { let _ = tx_backend.send(BackendMsg::Error(err.message)); }
                            EventMsg::TaskComplete(_) => { let _ = tx_backend.send(BackendMsg::TaskComplete); }
                            _ => {}
                        },
                        Err(e) => { let _ = tx_backend.send(BackendMsg::Error(format!("event error: {e}"))); break; }
                    }
                }
                msg = rx.recv() => {
                    match msg {
                        Some(FrontendMsg::SendPrompt(text)) => {
                            if !text.trim().is_empty() {
                                let _ = spawn_ok.codex.submit(AgentOp::UserInput { items: vec![AgentInputItem::Text { text }] }).await;
                            }
                        }
                        None => break,
                    }
                }
            }
        }

    });
}

#[derive(Clone, Debug)]
enum FrontendMsg {
    SendPrompt(String),
}

#[derive(Clone, Debug)]
enum BackendMsg {
    AgentText(String),
    AgentDelta(String),
    Reasoning(String),
    Error(String),
    TaskComplete,
    AuthMissing,
}

struct CodexGui {
    args: Args,
    to_backend: UnboundedSender<FrontendMsg>,
    rx_backend: UnboundedReceiver<BackendMsg>,
    // UI state
    prompt: String,
    transcript: Vec<String>,
    memory_items: Vec<String>,
    repo_root: PathBuf,
    recall_items: Vec<String>,
    reasoning_lines: Vec<String>,
    response_open: bool,
    response_text: String,
    auth_missing: bool,
    dark_mode: bool,
}

impl CodexGui {
    fn new(
        cc: &eframe::CreationContext<'_>,
        args: Args,
        to_backend: UnboundedSender<FrontendMsg>,
        rx_backend: UnboundedReceiver<BackendMsg>,
    ) -> Self {
        let repo_root = args
            .cwd
            .clone()
            .unwrap_or_else(|| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")));

        // Default to dark visuals; user can toggle at runtime.
        cc.egui_ctx.set_visuals(egui::Visuals::dark());

        let mut this = Self {
            args,
            to_backend,
            rx_backend,
            prompt: String::new(),
            transcript: Vec::new(),
            memory_items: Vec::new(),
            repo_root,
            recall_items: Vec::new(),
            reasoning_lines: Vec::new(),
            response_open: false,
            response_text: String::new(),
            auth_missing: false,
            dark_mode: true,
        };
        this.refresh_memory_safely();
        this
    }

    fn toggle_theme(&mut self, ctx: &egui::Context) {
        self.dark_mode = !self.dark_mode;
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }
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
                    item_cap: 8,
                    token_cap: 300,
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
        while let Ok(msg) = self.rx_backend.try_recv() {
            match msg {
                BackendMsg::AgentText(text) => {
                    if !text.is_empty() {
                        self.response_text = text.clone();
                        self.response_open = true;
                        self.transcript.push(format!("Codex: {text}"));
                    }
                }
                BackendMsg::AgentDelta(delta) => {
                    if !delta.is_empty() {
                        if self.response_text.is_empty() {
                            self.response_open = true;
                        }
                        self.response_text.push_str(&delta);
                    }
                }
                BackendMsg::Reasoning(r) => {
                    self.reasoning_lines.push(r);
                }
                BackendMsg::Error(e) => {
                    self.response_text = format!("Error: {e}");
                    self.response_open = true;
                }
                BackendMsg::TaskComplete => {}
                BackendMsg::AuthMissing => {
                    self.auth_missing = true;
                }
            }
        }
        // Theme toggle: Cmd/Ctrl+T
        if ctx.input(|i| i.key_pressed(egui::Key::T) && i.modifiers.command_only()) {
            self.toggle_theme(ctx);
        }
        if self.auth_missing {
            egui::TopBottomPanel::top("auth_banner").show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.colored_label(egui::Color32::from_rgb(200, 60, 60), "Not authenticated: set OPENAI_API_KEY or run `codex login`.");
                    ui.small("Tip: set an API key with `export OPENAI_API_KEY=sk-...` before launching the GUI.");
                });
            });
        }

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
                ui.separator();
                let theme_label = if self.dark_mode { "Light" } else { "Dark" };
                if ui.button(theme_label).clicked() {
                    self.toggle_theme(ctx);
                }
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
                && ui.input(|i| {
                    i.key_pressed(egui::Key::Enter)
                        && (i.modifiers.shift_only() || i.modifiers.command_only())
                })
            {
                self.to_backend
                    .send(FrontendMsg::SendPrompt(self.prompt.clone()))
                    .ok();
                self.transcript.push(format!("You: {}", self.prompt));
                let q = self.prompt.clone();
                self.perform_recall_safely(&q);
                self.response_text = if self.recall_items.is_empty() {
                    "(demo) No model wired yet; recall is shown at right.".into()
                } else {
                    let mut t = String::from(
                        "(demo) Relevant memory:
",
                    );
                    for it in &self.recall_items {
                        t.push_str(it);
                        t.push_str(
                            "
",
                        );
                    }
                    t
                };
                self.response_open = true;
                self.prompt.clear();
            }
            // Keyboard shortcuts for composer actions
            let save_shortcut =
                ui.input(|i| i.key_pressed(egui::Key::S) && i.modifiers.command_only());
            let recall_shortcut =
                ui.input(|i| i.key_pressed(egui::Key::R) && i.modifiers.command_only());
            let clear_shortcut =
                ui.input(|i| i.key_pressed(egui::Key::L) && i.modifiers.command_only());
            if save_shortcut {
                self.add_prompt_to_memory_safely();
            }
            if recall_shortcut {
                let q = self.prompt.clone();
                self.perform_recall_safely(&q);
            }
            if clear_shortcut {
                self.prompt.clear();
            }

            ui.horizontal(|ui| {
                if ui.button("Send (Shift/Ctrl+Enter)").clicked() {
                    self.to_backend
                        .send(FrontendMsg::SendPrompt(self.prompt.clone()))
                        .ok();
                    self.transcript.push(format!("You: {}", self.prompt));
                    let q = self.prompt.clone();
                    self.perform_recall_safely(&q);
                    self.response_text = if self.recall_items.is_empty() {
                        "(demo) No model wired yet; recall is shown at right.".into()
                    } else {
                        let mut t = String::from(
                            "(demo) Relevant memory:
",
                        );
                        for it in &self.recall_items {
                            t.push_str(it);
                            t.push_str(
                                "
",
                            );
                        }
                        t
                    };
                    self.response_open = true;
                    self.prompt.clear();
                }
                if ui.button("Save (Ctrl+S)").clicked() {
                    self.add_prompt_to_memory_safely();
                }
                if ui.button("Recall (Ctrl+R)").clicked() {
                    let q = self.prompt.clone();
                    self.perform_recall_safely(&q);
                }
                if ui.button("Refresh Memory").clicked() {
                    self.refresh_memory_safely();
                }
                if ui.button("Clear (Ctrl+L)").clicked() {
                    self.prompt.clear();
                }
            });
        });

        egui::SidePanel::left("reasoning_panel")
            .resizable(true)
            .default_width(280.0)
            .show(ctx, |ui| {
                ui.heading("Reasoning");
                egui::ScrollArea::vertical()
                    .id_source("reasoning_scroll")
                    .show(ui, |ui| {
                        for line in &self.reasoning_lines {
                            ui.label(line);
                            ui.separator();
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
                egui::ScrollArea::vertical()
                    .id_source("transcript_scroll")
                    .show(&mut cols[0], |ui| {
                        for line in &self.transcript {
                            ui.label(line);
                            ui.separator();
                        }
                    });

                cols[1].heading("Relevant Memory (Recall)");
                egui::ScrollArea::vertical()
                    .id_source("recall_scroll")
                    .show(&mut cols[1], |ui| {
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

        egui::Window::new("Response from Codex")
            .id(egui::Id::new("response_window"))
            .open(&mut self.response_open)
            .resizable(true)
            .show(ctx, |ui| {
                if self.response_text.is_empty() {
                    ui.label("No response yet.");
                } else {
                    ui.label(&self.response_text);
                }
            });
    }
}
