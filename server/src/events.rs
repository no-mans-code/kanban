use serde_json::json;
use tokio::sync::broadcast;

/// Fan-out of change notifications to every open SSE stream. Payloads say
/// *what* changed, not the new state; clients refetch what they display.
#[derive(Clone)]
pub struct Events {
    tx: broadcast::Sender<String>,
}

impl Default for Events {
    fn default() -> Self {
        let (tx, _) = broadcast::channel(1024);
        Events { tx }
    }
}

impl Events {
    pub fn emit(&self, kind: &str, project_id: Option<i64>, keys: &[String]) {
        let msg = json!({ "type": kind, "project_id": project_id, "keys": keys });
        // No subscribers is not an error.
        let _ = self.tx.send(msg.to_string());
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}
