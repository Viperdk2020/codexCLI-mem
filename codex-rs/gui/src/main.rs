use std::path::PathBuf;
use clap::Parser;
use eframe::egui;
use tokio::sync::mpsc::{unbounded_channel, UnboundedReceiver, UnboundedSender};
use tracing_subscriber::EnvFilter;

#[derive(clap::ValueEnum, Clone, Debug)]
enum MemoryToggle { On, Off, Auto }

#[derive(Parser, Debug)]
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
    // Logging
    let _ = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::from_default_env().add_directive("info".parse().unwrap()))
        .try_init();

    let args = Args::parse();
    let native_options = eframe::NativeOptions::default();

    let (tx, rx) = unbounded_channel();
    std::thread::spawn(move || backend_thread(rx));

    let _ = eframe::run_native(
        "Codex GUI",
        native_options,
        Box::new(|cc| Ok(Box::new(CodexGui::new(cc, args, tx)))),
    );
}

// Placeholder backend thread – will integrate codex-core events later.
fn backend_thread(_rx: UnboundedReceiver<FrontendMsg>) {
    // For MVP skeleton we do nothing here.
}

#[derive(Clone, Debug)]
enum FrontendMsg {
    SendPrompt(String),
}

struct CodexGui {
    args: Args,
    to_backend: UnboundedSender<FrontendMsg>,
    // UI state
    prompt: String,
    transcript: Vec<String>,
    memory_items: Vec<String>,
}

impl CodexGui {
    fn new(_cc: &eframe::CreationContext<'_>, args: Args, to_backend: UnboundedSender<FrontendMsg>) -> Self {
        Self { args, to_backend, prompt: String::new(), transcript: Vec::new(), memory_items: Vec::new() }
    }
}

impl eframe::App for CodexGui {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top_bar").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Codex GUI — MVP");
                ui.separator();
                ui.label(format!("cwd: {}", self.args.cwd.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "auto".into())));
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
            if r.response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && i.modifiers.shift_only()) {
                self.to_backend.send(FrontendMsg::SendPrompt(self.prompt.clone())).ok();
                self.transcript.push(format!("You: {}", self.prompt));
                self.prompt.clear();
            }
            ui.horizontal(|ui| {
                if ui.button("Send (Shift+Enter)").clicked() {
                    self.to_backend.send(FrontendMsg::SendPrompt(self.prompt.clone())).ok();
                    self.transcript.push(format!("You: {}", self.prompt));
                    self.prompt.clear();
                }
                if ui.button("Clear").clicked() { self.prompt.clear(); }
            });
        });

        egui::SidePanel::right("memory_panel").resizable(true).default_width(320.0).show(ctx, |ui| {
            ui.heading("Project Memory");
            if self.memory_items.is_empty() { ui.label("No durable items yet."); }
            for item in &self.memory_items {
                ui.label(item);
            }
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
    }
}
