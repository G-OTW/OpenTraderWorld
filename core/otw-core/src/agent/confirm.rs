//! Write consent (R5) — the user approves a mutation before it happens.
//!
//! The model asking to write is not the user agreeing to it. A run that is about to call
//! `otw_write` pauses, emits a `confirm` SSE frame describing the exact call, and waits for a
//! decision from the UI. Approve → the tool runs; decline or silence → the tool returns a
//! refusal the model can read, and nothing was mutated.
//!
//! Two things are deliberate:
//!
//! - **Waiting inside the run, not across turns.** The alternative — refuse, ask, re-ask on
//!   the next message — burns a turn, and the model has to reconstruct the call it wanted to
//!   make. Here the pending call is held verbatim and either runs or does not.
//! - **`auto_approve_writes` never covers a delete.** A user who ticked the box wanted to skip
//!   the chip on routine writes, not to hand over the power to erase their journal. Anything
//!   that destroys data confirms regardless of the flag.
//!
//! The registry is in-process: a restart drops every pending write, which is the safe
//! direction (nothing runs unconfirmed).

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;
use tokio::sync::oneshot;
use uuid::Uuid;

/// How long a pending write waits for a decision before it is treated as declined. Long
/// enough for a user who stepped away from the tab to come back and read the call.
pub const WAIT: Duration = Duration::from_secs(300);

/// One write the model wants to make, as shown to the user.
pub struct WriteIntent {
    pub method: String,
    pub path: String,
    pub body: Option<Value>,
    /// Destroys data. Always confirmed, whatever `auto_approve_writes` says.
    pub destructive: bool,
}

impl WriteIntent {
    /// The `confirm` frame payload.
    pub fn to_json(&self, tool_use_id: &str) -> Value {
        serde_json::json!({
            "id": tool_use_id,
            "method": self.method,
            "path": self.path,
            "body": self.body,
            "destructive": self.destructive,
        })
    }
}

type Key = (Uuid, String);

static PENDING: OnceLock<Mutex<HashMap<Key, oneshot::Sender<bool>>>> = OnceLock::new();

fn pending() -> &'static Mutex<HashMap<Key, oneshot::Sender<bool>>> {
    PENDING.get_or_init(|| Mutex::new(HashMap::new()))
}

/// How a pending write ended. The three cases read differently to the model: an approval
/// runs the call, a decline is a decision it must report, and a timeout is the user never
/// having seen the question. Collapsing the last two (every non-approval reported as
/// "no answer in time") makes a refusal look like a transport failure, and the model
/// retries the write it was just refused.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Decision {
    Approved,
    Declined,
    TimedOut,
}

/// A registered wait. Dropping it deregisters the key, so an aborted run never leaves a
/// pending write the UI could still answer.
pub struct Pending {
    key: Key,
    rx: Option<oneshot::Receiver<bool>>,
}

/// Register a pending write for `tool_use_id` in this conversation.
pub fn register(conversation_id: Uuid, tool_use_id: &str) -> Pending {
    let (tx, rx) = oneshot::channel();
    let key = (conversation_id, tool_use_id.to_string());
    pending().lock().unwrap().insert(key.clone(), tx);
    Pending { key, rx: Some(rx) }
}

impl Pending {
    /// Block until the user decides, or [`WAIT`] elapses. A dropped sender counts as a
    /// timeout: nobody answered. The key is deregistered on drop, which happens either way.
    pub async fn wait(mut self) -> Decision {
        let Some(rx) = self.rx.take() else {
            return Decision::TimedOut;
        };
        match tokio::time::timeout(WAIT, rx).await {
            Ok(Ok(true)) => Decision::Approved,
            Ok(Ok(false)) => Decision::Declined,
            Ok(Err(_)) | Err(_) => Decision::TimedOut,
        }
    }
}

impl Drop for Pending {
    fn drop(&mut self) {
        pending().lock().unwrap().remove(&self.key);
    }
}

/// Answer a pending write. `false` when there is nothing waiting under that id — a stale
/// click, or a run that already timed out.
pub fn resolve(conversation_id: Uuid, tool_use_id: &str, approve: bool) -> bool {
    let key = (conversation_id, tool_use_id.to_string());
    let Some(tx) = pending().lock().unwrap().remove(&key) else {
        return false;
    };
    tx.send(approve).is_ok()
}

/// What the model is told when a write does not happen. Phrased so the model reports the
/// outcome rather than retrying the same call under a different name.
pub fn declined_message(intent: &WriteIntent, decision: Decision) -> String {
    let why = match decision {
        Decision::Declined => {
            "the user saw this exact call in the confirmation prompt and REFUSED it. That is \
             their decision, not a system error — say so in those terms"
        }
        _ => "the user did not answer in time",
    };
    format!(
        "the write was NOT performed: {why}. Nothing was changed by `{} {}`. Do not retry this \
         call and do not describe this as a server, network or timeout failure — tell the user \
         it was not applied, and ask what they want instead.",
        intent.method, intent.path
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn approve_resolves_the_wait() {
        let conv = Uuid::new_v4();
        let p = register(conv, "tool-1");
        assert!(resolve(conv, "tool-1", true));
        assert_eq!(p.wait().await, Decision::Approved);
    }

    /// A decline must stay distinguishable from silence all the way to the model.
    #[tokio::test]
    async fn decline_is_not_a_timeout() {
        let conv = Uuid::new_v4();
        let p = register(conv, "tool-2");
        assert!(resolve(conv, "tool-2", false));
        assert_eq!(p.wait().await, Decision::Declined);
    }

    /// A decision for another conversation must never release this one — the tool_use id
    /// only has to be unique within a provider turn, not globally.
    #[tokio::test]
    async fn resolution_is_scoped_to_the_conversation() {
        let a = Uuid::new_v4();
        let b = Uuid::new_v4();
        let p = register(a, "same-id");
        assert!(!resolve(b, "same-id", true));
        drop(p);
    }

    /// Nothing waiting: a stale click is a no-op, not a panic.
    #[test]
    fn resolving_an_unknown_write_is_a_no_op() {
        assert!(!resolve(Uuid::new_v4(), "nope", true));
    }

    /// The frame the UI renders. Approving a summary of a call is not consent to the call, so
    /// the exact method, path and body all have to travel.
    #[test]
    fn the_frame_carries_the_whole_call() {
        let intent = WriteIntent {
            method: "DELETE".into(),
            path: "/api/journal/trades/7".into(),
            body: Some(serde_json::json!({ "confirm": true })),
            destructive: true,
        };
        let v = intent.to_json("tool-42");
        assert_eq!(v["id"], "tool-42");
        assert_eq!(v["method"], "DELETE");
        assert_eq!(v["path"], "/api/journal/trades/7");
        assert_eq!(v["body"], serde_json::json!({ "confirm": true }));
        assert_eq!(v["destructive"], true);
    }

    /// The model must read a decline as a decision, not as a transport error to retry around.
    #[test]
    fn the_decline_message_forbids_a_retry() {
        let intent = WriteIntent {
            method: "POST".into(),
            path: "/api/journal/trades".into(),
            body: None,
            destructive: false,
        };
        for d in [Decision::Declined, Decision::TimedOut] {
            let msg = declined_message(&intent, d);
            assert!(msg.contains("NOT performed"), "{msg}");
            assert!(msg.contains("Do not retry"), "{msg}");
            assert!(msg.contains("/api/journal/trades"), "{msg}");
        }
        assert!(declined_message(&intent, Decision::TimedOut).contains("did not answer"));
        let refused = declined_message(&intent, Decision::Declined);
        assert!(refused.contains("REFUSED"), "{refused}");
        assert!(!refused.contains("did not answer"), "a refusal is not a timeout: {refused}");
    }

    /// Dropping the wait (aborted run) must clear the entry, or the UI would report success
    /// for a write that can no longer happen.
    #[test]
    fn dropping_deregisters() {
        let conv = Uuid::new_v4();
        let p = register(conv, "tool-3");
        drop(p);
        assert!(!resolve(conv, "tool-3", true));
    }
}
