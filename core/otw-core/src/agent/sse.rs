//! Minimal SSE line decoder for provider byte streams.
//!
//! Both adapters consume `text/event-stream`: newline-delimited frames where lines beginning
//! `data:` carry the payload and a blank line ends an event. Providers may split a frame
//! across TCP chunks, so we buffer until a complete `data:` line is seen. `event:` type lines
//! are surfaced separately (Anthropic uses them; OpenAI-compat does not).

/// Accumulates bytes and yields complete `data:` payloads as they arrive.
pub struct SseDecoder {
    buf: String,
}

impl SseDecoder {
    pub fn new() -> Self {
        Self { buf: String::new() }
    }

    /// Feed a chunk; return every complete `data:` payload it completes (may be several,
    /// may be none). Only whole lines (terminated by `\n`) are consumed.
    pub fn push(&mut self, bytes: &[u8]) -> Vec<String> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if let Some(rest) = line.strip_prefix("data:") {
                out.push(rest.trim_start().to_string());
            }
        }
        out
    }
}

/// Same as [`SseDecoder`] but preserves the `event:` type line alongside each `data:` payload
/// (Anthropic's stream keys behavior off the event name). Yields `(event_type, data)` pairs;
/// `event_type` is empty until an `event:` line has been seen for the current frame.
pub struct SseTypedDecoder {
    buf: String,
    current_event: String,
}

impl SseTypedDecoder {
    pub fn new() -> Self {
        Self { buf: String::new(), current_event: String::new() }
    }

    pub fn push(&mut self, bytes: &[u8]) -> Vec<(String, String)> {
        self.buf.push_str(&String::from_utf8_lossy(bytes));
        let mut out = Vec::new();
        while let Some(nl) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=nl).collect();
            let line = line.trim_end_matches(['\r', '\n']);
            if line.is_empty() {
                self.current_event.clear();
            } else if let Some(rest) = line.strip_prefix("event:") {
                self.current_event = rest.trim().to_string();
            } else if let Some(rest) = line.strip_prefix("data:") {
                out.push((self.current_event.clone(), rest.trim_start().to_string()));
            }
        }
        out
    }
}
