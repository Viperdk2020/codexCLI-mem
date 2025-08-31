use codex_core::config::Config;
use codex_core::protocol::Event;
use codex_core::protocol::EventMsg;
use codex_core::protocol::Op;
use codex_core::ConversationManager;
use codex_core::NewConversation;
use std::sync::Arc;
use tokio::sync::mpsc::unbounded_channel;
use tokio::sync::mpsc::UnboundedReceiver;
use tokio::sync::mpsc::UnboundedSender;

/// Spawn the codex_core conversation loops and return channels for
/// submitting Ops and receiving Events.
pub(crate) fn spawn_bridge(
    config: Config,
    server: Arc<ConversationManager>,
) -> (UnboundedSender<Op>, UnboundedReceiver<Event>) {
    let (op_tx, mut op_rx) = unbounded_channel::<Op>();
    let (event_tx, event_rx) = unbounded_channel::<Event>();

    tokio::spawn(async move {
        let NewConversation {
            conversation,
            session_configured,
            ..
        } = match server.new_conversation(config).await {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("failed to initialize codex-core: {e}");
                return;
            }
        };

        // Forward the captured SessionConfigured event first.
        let ev = Event {
            id: String::new(),
            msg: EventMsg::SessionConfigured(session_configured),
        };
        let _ = event_tx.send(ev);

        let convo_clone = conversation.clone();
        tokio::spawn(async move {
            while let Some(op) = op_rx.recv().await {
                if let Err(e) = convo_clone.submit(op).await {
                    tracing::error!("failed to submit op: {e}");
                }
            }
        });

        while let Ok(ev) = conversation.next_event().await {
            let _ = event_tx.send(ev);
        }
    });

    (op_tx, event_rx)
}
