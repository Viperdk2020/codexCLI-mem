mod bridge;
mod memory;

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use clap::Parser;
use codex_core::config::Config;
use codex_core::config::ConfigOverrides;
use codex_core::protocol::AskForApproval;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::protocol::ReviewDecision;
use codex_core::protocol::TokenUsage;
use codex_core::ConversationManager;
use codex_login::AuthManager;
use eframe::egui;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;
use tracing_subscriber::EnvFilter;

use memory::MemoryEntry;
use memory::MemoryLogger;

#[derive(clap::ValueEnum, Clone, Debug)]
enum MemoryToggle {
    On,
    Off,
    Auto,
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
}

fn main() {
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();

    let args = Args::parse();
    let native_options = eframe::NativeOptions::default();

    let (to_backend, from_frontend_rx) = unbounded_channel();
    let (from_backend_tx, from_backend_rx) = unbounded_channel();
    let args_clone = args.clone();
    std::thread::spawn(move || backend_thread(args_clone, from_frontend_rx, from_backend_tx));

    let _ = eframe::run_native(
        "Codex GUI",
        native_options,
        Box::new(|cc| {
            Ok(Box::new(CodexGui::new(
                cc,
                args,
                to_backend,
                from_backend_rx,
            )))
        }),
    );
}

#[derive(Clone, Debug)]
enum FrontendMsg {
    UserPrompt(String),
    AddPref(String),
    ApproveExec {
        id: String,
        decision: ReviewDecision,
    },
    ApprovePatch {
        id: String,
        decision: ReviewDecision,
    },
}

#[derive(Debug)]
enum BackendMsg {
    Event(Event),
    Memory {
        durable: Vec<MemoryEntry>,
        recent: Vec<MemoryEntry>,
    },
    Preamble(String),
    Status {
        model: String,
        reasoning: String,
        approval: AskForApproval,
        context: Option<u64>,
    },
}

fn should_enable_memory(args: &Args) -> bool {
    if let Ok(v) = std::env::var("CODEX_PER_REPO_MEMORY").or_else(|_| std::env::var("CODEX_MEMORY"))
    {
        let v = v.to_ascii_lowercase();
        if v == "0" || v == "off" {
            return false;
        }
        if v == "1" || v == "on" {
            return true;
        }
    }
    match args.memory {
        MemoryToggle::On => true,
        MemoryToggle::Off => false,
        MemoryToggle::Auto => true,
    }
}

fn backend_thread(
    args: Args,
    mut rx: UnboundedReceiver<FrontendMsg>,
    tx: UnboundedSender<BackendMsg>,
) {
    let rt = tokio::runtime::Runtime::new().expect("tokio runtime");
    rt.block_on(async move {
        let overrides = ConfigOverrides { cwd: args.cwd.clone(), ..Default::default() };
        let config = Config::load_with_cli_overrides(vec![], overrides).expect("config");
        let auth_manager = AuthManager::shared(config.codex_home.clone(), config.preferred_auth_method);
        let server = Arc::new(ConversationManager::new(auth_manager));
        let (op_tx, mut event_rx) = bridge::spawn_bridge(config.clone(), server);

        let _ = tx.send(BackendMsg::Status {
            model: config.model.clone(),
            reasoning: format!("{:?}", config.model_reasoning_effort),
            approval: config.approval_policy,
            context: config.model_context_window,
        });

        let mut memory_logger = if should_enable_memory(&args) {
            Some(MemoryLogger::new(args.cwd.clone().unwrap_or_else(|| std::env::current_dir().unwrap())))
        } else { None };

        if let Some(ref ml) = memory_logger {
            if let Some(pre) = ml.build_durable_preamble(512) {
                let _ = tx.send(BackendMsg::Preamble(pre));
            }
        }

        if let Some(ref ml) = memory_logger {
            let path = ml.memory_file.clone();
            let tx_mem = tx.clone();
            tokio::spawn(async move {
                loop {
                    let (durable, recent) = memory::read_memory_items(&path, 50);
                    let _ = tx_mem.send(BackendMsg::Memory { durable, recent });
                    tokio::time::sleep(Duration::from_secs(1)).await;
                }
            });
        }

        let mut preamble_injected = false;
        loop {
            tokio::select! {
                Some(ev) = event_rx.recv() => {
                    if let EventMsg::SessionConfigured(sc) = &ev.msg {
                        if let Some(ref mut ml) = memory_logger { ml.set_session_id(sc.session_id); }
                    }
                    let _ = tx.send(BackendMsg::Event(ev));
                }
                Some(msg) = rx.recv() => {
                    match msg {
                        FrontendMsg::UserPrompt(mut text) => {
                            if !preamble_injected {
                                if let Some(ref ml) = memory_logger {
                                    if let Some(pre) = ml.build_durable_preamble(512) {
                                        text = format!("{pre}\n{text}");
                                    }
                                }
                                preamble_injected = true;
                            }
                            let items = vec![codex_core::protocol::InputItem::Text { text: text.clone() }];
                            let _ = op_tx.send(Op::UserInput { items });
                            let _ = op_tx.send(Op::AddToHistory { text });
                        }
                        FrontendMsg::AddPref(s) => { if let Some(ref ml) = memory_logger { let _ = ml.add_pref(&s); } }
                        FrontendMsg::ApproveExec { id, decision } => { let _ = op_tx.send(Op::ExecApproval { id, decision }); }
                        FrontendMsg::ApprovePatch { id, decision } => { let _ = op_tx.send(Op::PatchApproval { id, decision }); }
                    }
                }
                else => break,
            }
        }
    });
}

#[derive(Clone)]
struct ApprovalRequest {
    id: String,
    kind: ApprovalKind,
    reason: Option<String>,
}

#[derive(Clone)]
enum ApprovalKind {
    Exec { command: Vec<String>, cwd: PathBuf },
    Patch { files: Vec<PathBuf> },
}

struct Status {
    model: String,
    reasoning: String,
    approval: AskForApproval,
    context: Option<u64>,
    tokens: TokenUsage,
}

struct CodexGui {
    to_backend: UnboundedSender<FrontendMsg>,
    from_backend: UnboundedReceiver<BackendMsg>,
    prompt: String,
    transcript: Vec<String>,
    durable: Vec<MemoryEntry>,
    recent: Vec<MemoryEntry>,
    new_pref: String,
    preamble: Option<String>,
    pending: Option<ApprovalRequest>,
    status: Status,
}

impl CodexGui {
    fn new(
        _cc: &eframe::CreationContext<'_>,
        _args: Args,
        to_backend: UnboundedSender<FrontendMsg>,
        from_backend: UnboundedReceiver<BackendMsg>,
    ) -> Self {
        Self {
            to_backend,
            from_backend,
            prompt: String::new(),
            transcript: Vec::new(),
            durable: Vec::new(),
            recent: Vec::new(),
            new_pref: String::new(),
            preamble: None,
            pending: None,
            status: Status {
                model: String::new(),
                reasoning: String::new(),
                approval: AskForApproval::default(),
                context: None,
                tokens: TokenUsage {
                    input_tokens: 0,
                    cached_input_tokens: None,
                    output_tokens: 0,
                    reasoning_output_tokens: None,
                    total_tokens: 0,
                },
            },
        }
    }

    fn handle_event(&mut self, ev: Event) {
        match ev.msg {
            EventMsg::AgentMessage(m) => self.transcript.push(format!("Codex: {}", m.message)),
            EventMsg::AgentMessageDelta(d) => {
                if let Some(last) = self.transcript.last_mut() {
                    last.push_str(&d.delta);
                } else {
                    self.transcript.push(d.delta);
                }
            }
            EventMsg::AgentReasoning(r) => self.transcript.push(format!("[thinking] {}", r.text)),
            EventMsg::TaskStarted(_) => self.transcript.push("Task started".into()),
            EventMsg::TaskComplete(_) => self.transcript.push("Task complete".into()),
            EventMsg::ExecApprovalRequest(req) => {
                self.pending = Some(ApprovalRequest {
                    id: ev.id,
                    kind: ApprovalKind::Exec {
                        command: req.command,
                        cwd: req.cwd,
                    },
                    reason: req.reason,
                });
            }
            EventMsg::ApplyPatchApprovalRequest(req) => {
                let files = req.changes.keys().cloned().collect();
                self.pending = Some(ApprovalRequest {
                    id: ev.id,
                    kind: ApprovalKind::Patch { files },
                    reason: req.reason,
                });
            }
            EventMsg::McpToolCallBegin(ev) => self.transcript.push(format!(
                "MCP start: {}.{}",
                ev.invocation.server, ev.invocation.tool
            )),
            EventMsg::McpToolCallEnd(ev) => self.transcript.push(format!(
                "MCP end: {}.{}",
                ev.invocation.server, ev.invocation.tool
            )),
            EventMsg::TokenCount(t) => self.status.tokens = t,
            EventMsg::Error(e) => self.transcript.push(format!("Error: {}", e.message)),
            EventMsg::SessionConfigured(_) => {}
            _ => {}
        }
    }
}

impl eframe::App for CodexGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        while let Ok(msg) = self.from_backend.try_recv() {
            match msg {
                BackendMsg::Event(ev) => {
                    self.handle_event(ev);
                    ctx.request_repaint();
                }
                BackendMsg::Memory { durable, recent } => {
                    self.durable = durable;
                    self.recent = recent;
                }
                BackendMsg::Preamble(p) => {
                    self.preamble = Some(p);
                }
                BackendMsg::Status {
                    model,
                    reasoning,
                    approval,
                    context,
                } => {
                    self.status.model = model;
                    self.status.reasoning = reasoning;
                    self.status.approval = approval;
                    self.status.context = context;
                }
            }
        }

        egui::TopBottomPanel::top("status_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!("Model: {}", self.status.model));
                ui.separator();
                if let Some(ctxw) = self.status.context {
                    ui.label(format!(
                        "Tokens: {}/{}",
                        self.status.tokens.total_tokens, ctxw
                    ));
                } else {
                    ui.label(format!("Tokens used: {}", self.status.tokens.total_tokens));
                }
                ui.separator();
                ui.label(format!("Approval: {:?}", self.status.approval));
                ui.separator();
                ui.label(format!("Reasoning: {}", self.status.reasoning));
            });
        });

        egui::SidePanel::right("memory_panel")
            .resizable(true)
            .default_width(320.0)
            .show(ctx, |ui| {
                ui.heading("Project Memory");
                ui.separator();
                ui.label("Durable:");
                if self.durable.is_empty() {
                    ui.label("(none)");
                }
                for item in &self.durable {
                    let prefix = item.id.chars().take(8).collect::<String>();
                    ui.label(format!("{} {}: {}", prefix, item.r#type, item.content));
                }
                ui.separator();
                ui.label("Recent:");
                if self.recent.is_empty() {
                    ui.label("(none)");
                }
                for item in &self.recent {
                    ui.label(format!("{}: {}", item.r#type, item.content));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.new_pref).hint_text("Add pref…"));
                    if ui.button("Add Pref").clicked()
                        && !self.new_pref.is_empty() {
                            self.to_backend
                                .send(FrontendMsg::AddPref(self.new_pref.clone()))
                                .ok();
                            self.new_pref.clear();
                        }
                });
            });

        egui::TopBottomPanel::bottom("composer").show(ctx, |ui| {
            if let Some(pre) = &self.preamble {
                ui.label(pre);
                ui.separator();
            }
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
                    .send(FrontendMsg::UserPrompt(self.prompt.clone()))
                    .ok();
                self.transcript.push(format!("You: {}", self.prompt));
                self.prompt.clear();
                self.preamble = None;
            }
            ui.horizontal(|ui| {
                if ui.button("Send (Shift+Enter)").clicked() {
                    self.to_backend
                        .send(FrontendMsg::UserPrompt(self.prompt.clone()))
                        .ok();
                    self.transcript.push(format!("You: {}", self.prompt));
                    self.prompt.clear();
                    self.preamble = None;
                }
                if ui.button("Clear").clicked() {
                    self.prompt.clear();
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Transcript");
            egui::ScrollArea::vertical().show(ui, |ui| {
                for line in &self.transcript {
                    ui.label(line);
                    ui.separator();
                }
            });
        });

        if let Some(app) = self.pending.clone() {
            egui::Window::new("Approval required")
                .collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    match &app.kind {
                        ApprovalKind::Exec { command, cwd } => {
                            ui.label(format!("Command: {}", command.join(" ")));
                            ui.label(format!("Cwd: {}", cwd.display()));
                        }
                        ApprovalKind::Patch { files } => {
                            ui.label(format!(
                                "Files: {}",
                                files
                                    .iter()
                                    .map(|f| f.display().to_string())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ));
                        }
                    }
                    if let Some(reason) = &app.reason {
                        ui.label(format!("Reason: {}", reason));
                    }
                    ui.horizontal(|ui| {
                        if ui.button("Approve").clicked() {
                            match &app.kind {
                                ApprovalKind::Exec { .. } => {
                                    self.to_backend
                                        .send(FrontendMsg::ApproveExec {
                                            id: app.id.clone(),
                                            decision: ReviewDecision::Approved,
                                        })
                                        .ok();
                                }
                                ApprovalKind::Patch { .. } => {
                                    self.to_backend
                                        .send(FrontendMsg::ApprovePatch {
                                            id: app.id.clone(),
                                            decision: ReviewDecision::Approved,
                                        })
                                        .ok();
                                }
                            }
                            self.transcript.push("System: approved".into());
                            self.pending = None;
                        }
                        if ui.button("Deny").clicked() {
                            match &app.kind {
                                ApprovalKind::Exec { .. } => {
                                    self.to_backend
                                        .send(FrontendMsg::ApproveExec {
                                            id: app.id.clone(),
                                            decision: ReviewDecision::Denied,
                                        })
                                        .ok();
                                }
                                ApprovalKind::Patch { .. } => {
                                    self.to_backend
                                        .send(FrontendMsg::ApprovePatch {
                                            id: app.id.clone(),
                                            decision: ReviewDecision::Denied,
                                        })
                                        .ok();
                                }
                            }
                            self.transcript.push("System: denied".into());
                            self.pending = None;
                        }
                    });
                });
        }
    }
}
