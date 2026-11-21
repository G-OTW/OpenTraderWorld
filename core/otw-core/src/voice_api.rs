//! HTTP API for voice control: settings, speech engines (the voice broker), spoken
//! commands, and the transcription relay.
//!
//! The browser records, converts to 16 kHz mono WAV and posts the bytes here; this relays
//! them to the selected engine and returns the text. Turning text into actions happens in
//! the browser, which already owns navigation and the agent widget, so nothing here runs
//! a command. Not in the MCP catalog: an engine holds a key, and transcription spends it.

use std::time::{Duration, Instant};

use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Path, Query, State},
    http::HeaderMap,
    routing::{get, patch, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::{json, Value};
use uuid::Uuid;

use otw_store::voice::{
    self as store, CommandInput, EngineInput, VoiceSettings, BROWSER_ENGINE, ENGINE_KINDS,
};

use crate::{ApiError, AppState};

/// One utterance. A minute of 16 kHz mono 16-bit WAV is under 2 MB; the cap leaves room for
/// a longer dictation without accepting arbitrary uploads.
const MAX_AUDIO_BYTES: usize = 8 * 1024 * 1024;

/// An engine that has not answered by then is not going to answer in time for a person
/// holding a key.
const TRANSCRIBE_TIMEOUT: Duration = Duration::from_secs(60);

/// A command is a short macro, not a workflow: longer chains belong in the Automator.
const MAX_STEPS: usize = 20;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/voice/settings", get(get_settings).put(put_settings))
        .route("/api/voice/engines", get(list_engines).post(add_engine))
        .route("/api/voice/engines/{id}", patch(update_engine).delete(delete_engine))
        .route("/api/voice/engines/{id}/test", post(test_engine))
        .route("/api/voice/commands", get(list_commands).post(add_command))
        .route("/api/voice/commands/reorder", post(reorder_commands))
        .route("/api/voice/commands/{id}", patch(update_command).delete(delete_command))
        .route(
            "/api/voice/transcribe",
            post(transcribe).layer(DefaultBodyLimit::max(MAX_AUDIO_BYTES)),
        )
}

// ── Settings ──────────────────────────────────────────────────────────────────

async fn get_settings(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "settings": store::get_settings(&state.pool).await? })))
}

async fn put_settings(
    State(state): State<AppState>,
    Json(mut input): Json<VoiceSettings>,
) -> Result<Json<Value>, ApiError> {
    input.engine = input.engine.trim().to_string();
    input.language = input.language.trim().to_string();
    input.shortcut = input.shortcut.trim().to_string();
    input.dictation_shortcut = input.dictation_shortcut.trim().to_string();
    if !input.engine.is_empty() && input.engine != BROWSER_ENGINE {
        let id = Uuid::parse_str(&input.engine)
            .map_err(|_| ApiError::bad_request("unknown speech engine"))?;
        if store::get_engine(&state.pool, id).await?.is_none() {
            return Err(ApiError::bad_request("that speech engine no longer exists"));
        }
    }
    if input.language.len() > 16 {
        return Err(ApiError::bad_request("language must be a short code such as 'en' or 'fr-FR'"));
    }
    if input.shortcut.is_empty() || input.dictation_shortcut.is_empty() {
        return Err(ApiError::bad_request("both push-to-talk shortcuts need a key"));
    }
    if input.shortcut == input.dictation_shortcut {
        return Err(ApiError::bad_request(
            "the command and dictation shortcuts must differ, or one press could not tell them apart",
        ));
    }
    store::set_settings(&state.pool, &input).await?;
    Ok(Json(json!({ "settings": input })))
}

// ── Engines ───────────────────────────────────────────────────────────────────

fn validate_engine(input: &mut EngineInput) -> Result<(), ApiError> {
    input.label = input.label.trim().to_string();
    input.base_url = input.base_url.trim().trim_end_matches('/').to_string();
    input.model = input.model.trim().to_string();
    if !ENGINE_KINDS.contains(&input.kind.as_str()) {
        return Err(ApiError::bad_request(&format!(
            "engine kind must be one of {}",
            ENGINE_KINDS.join(", ")
        )));
    }
    if input.label.is_empty() {
        return Err(ApiError::bad_request("give the engine a name"));
    }
    if !(input.base_url.starts_with("http://") || input.base_url.starts_with("https://")) {
        return Err(ApiError::bad_request("the server URL must start with http:// or https://"));
    }
    if input.kind == "openai_compat" && input.model.is_empty() {
        return Err(ApiError::bad_request(
            "name the transcription model, e.g. whisper-1 or Systran/faster-whisper-small",
        ));
    }
    Ok(())
}

async fn list_engines(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({
        "engines": store::list_engines(&state.pool).await?,
        "kinds": ENGINE_KINDS,
    })))
}

async fn add_engine(
    State(state): State<AppState>,
    Json(mut input): Json<EngineInput>,
) -> Result<Json<Value>, ApiError> {
    validate_engine(&mut input)?;
    let engine = store::add_engine(&state.pool, &state.cipher, &input).await?;
    Ok(Json(json!({ "engine": engine })))
}

async fn update_engine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<EngineInput>,
) -> Result<Json<Value>, ApiError> {
    validate_engine(&mut input)?;
    let engine = store::update_engine(&state.pool, &state.cipher, id, &input)
        .await?
        .ok_or_else(|| ApiError::not_found("speech engine not found"))?;
    Ok(Json(json!({ "engine": engine })))
}

async fn delete_engine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_engine(&state.pool, id).await? {
        return Err(ApiError::not_found("speech engine not found"));
    }
    // An engine in use leaves voice control with nothing to send audio to: say so on the
    // settings page rather than failing on the next press.
    let mut s = store::get_settings(&state.pool).await?;
    if s.engine == id.to_string() {
        s.engine.clear();
        store::set_settings(&state.pool, &s).await?;
    }
    Ok(Json(json!({ "ok": true })))
}

/// Send half a second of silence: proves the URL, the key and the model in one round trip.
async fn test_engine(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    let engine = store::get_engine(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::not_found("speech engine not found"))?;
    let started = Instant::now();
    relay(&state, &engine, silence_wav(), "").await?;
    Ok(Json(json!({ "ok": true, "ms": started.elapsed().as_millis() as u64 })))
}

// ── Transcription ─────────────────────────────────────────────────────────────

#[derive(Deserialize)]
struct TranscribeQuery {
    /// Engine id; absent uses the one picked in the settings.
    #[serde(default)]
    engine: Option<String>,
    #[serde(default)]
    lang: Option<String>,
}

async fn transcribe(
    State(state): State<AppState>,
    Query(q): Query<TranscribeQuery>,
    headers: HeaderMap,
    body: Bytes,
) -> Result<Json<Value>, ApiError> {
    let ctype = headers
        .get(axum::http::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default();
    if !ctype.starts_with("audio/wav") && !ctype.starts_with("audio/x-wav") {
        return Err(ApiError::bad_request("send the recording as audio/wav"));
    }
    if body.len() < 64 {
        return Err(ApiError::bad_request("the recording is empty"));
    }
    let settings = store::get_settings(&state.pool).await?;
    let wanted = q.engine.filter(|e| !e.is_empty()).unwrap_or(settings.engine.clone());
    if wanted.is_empty() {
        return Err(ApiError::bad_request(
            "no speech engine selected: pick one in Settings → Voice → Speech engine",
        ));
    }
    if wanted == BROWSER_ENGINE {
        return Err(ApiError::bad_request(
            "the browser engine transcribes in the browser, there is nothing to send",
        ));
    }
    let id = Uuid::parse_str(&wanted).map_err(|_| ApiError::bad_request("unknown speech engine"))?;
    let engine = store::get_engine(&state.pool, id)
        .await?
        .ok_or_else(|| ApiError::bad_request("the selected speech engine no longer exists"))?;
    let lang = q.lang.unwrap_or(settings.language);
    let started = Instant::now();
    let text = relay(&state, &engine, body.to_vec(), &lang).await?;
    Ok(Json(json!({ "text": text, "ms": started.elapsed().as_millis() as u64 })))
}

/// Post one WAV to an engine and return its text.
async fn relay(
    state: &AppState,
    engine: &store::Engine,
    wav: Vec<u8>,
    lang: &str,
) -> Result<String, ApiError> {
    let key = store::engine_key(&state.pool, &state.cipher, engine.id).await?;
    // Both server shapes take an ISO-639-1 code; a regional suffix is dropped.
    let lang = lang.split(['-', '_']).next().unwrap_or_default().to_lowercase();
    let mut form = Multipart::new();
    form.file("file", "speech.wav", "audio/wav", wav);
    form.text("response_format", "json");
    let url = match engine.kind.as_str() {
        "openai_compat" => {
            form.text("model", &engine.model);
            if !lang.is_empty() {
                form.text("language", &lang);
            }
            format!("{}/audio/transcriptions", engine.base_url)
        }
        "whisper_cpp" => {
            form.text("language", if lang.is_empty() { "auto" } else { &lang });
            form.text("temperature", "0");
            format!("{}/inference", engine.base_url)
        }
        other => return Err(ApiError::bad_request(&format!("unknown engine kind '{other}'"))),
    };
    let (ctype, body) = form.finish();
    let mut req = state
        .http
        .post(&url)
        .timeout(TRANSCRIBE_TIMEOUT)
        .header(reqwest::header::CONTENT_TYPE, ctype)
        .body(body);
    if !key.is_empty() {
        req = req.bearer_auth(key);
    }
    let res = req.send().await.map_err(|e| {
        let why = if e.is_timeout() {
            "timed out".to_string()
        } else if e.is_connect() {
            "is unreachable".to_string()
        } else {
            format!("failed ({e})")
        };
        ApiError::bad_gateway(&format!("speech engine '{}' at {url} {why}", engine.label))
    })?;
    let status = res.status();
    let raw = res.text().await.unwrap_or_default();
    if !status.is_success() {
        let snippet: String = raw.chars().take(240).collect();
        return Err(ApiError::bad_gateway(&format!(
            "speech engine '{}' answered {}: {}",
            engine.label,
            status.as_u16(),
            snippet.trim()
        )));
    }
    let v: Value = serde_json::from_str(&raw).map_err(|_| {
        ApiError::bad_gateway(&format!(
            "speech engine '{}' did not answer with JSON; check the URL points at the API",
            engine.label
        ))
    })?;
    let text = v
        .get("text")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            ApiError::bad_gateway(&format!("speech engine '{}' returned no text", engine.label))
        })?;
    Ok(text.trim().to_string())
}

/// Minimal multipart/form-data body. reqwest's own needs a feature this workspace does not
/// pull in, and an upload this size does not need streaming.
struct Multipart {
    boundary: String,
    body: Vec<u8>,
}

impl Multipart {
    fn new() -> Self {
        Self { boundary: format!("otw-voice-{}", Uuid::new_v4().simple()), body: Vec::new() }
    }

    fn text(&mut self, name: &str, value: &str) {
        self.body.extend_from_slice(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{name}\"\r\n\r\n{value}\r\n",
                self.boundary
            )
            .as_bytes(),
        );
    }

    fn file(&mut self, name: &str, filename: &str, ctype: &str, bytes: Vec<u8>) {
        self.body.extend_from_slice(
            format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{name}\"; filename=\"{filename}\"\r\n\
                 Content-Type: {ctype}\r\n\r\n",
                self.boundary
            )
            .as_bytes(),
        );
        self.body.extend_from_slice(&bytes);
        self.body.extend_from_slice(b"\r\n");
    }

    fn finish(mut self) -> (String, Vec<u8>) {
        self.body.extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        (format!("multipart/form-data; boundary={}", self.boundary), self.body)
    }
}

/// Half a second of 16 kHz mono 16-bit silence, as a WAV file.
fn silence_wav() -> Vec<u8> {
    const RATE: u32 = 16_000;
    let samples = RATE / 2;
    let data_len = samples * 2;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&(RATE * 2).to_le_bytes());
    out.extend_from_slice(&2u16.to_le_bytes());
    out.extend_from_slice(&16u16.to_le_bytes());
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    out.resize(44 + data_len as usize, 0);
    out
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Lowercase, letters and digits only, single spaces: the form two phrases are compared in.
fn norm(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_alphanumeric() { c.to_lowercase().next().unwrap_or(c) } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn str_field<'a>(step: &'a Value, key: &str) -> &'a str {
    step.get(key).and_then(Value::as_str).unwrap_or_default().trim()
}

fn validate_steps(steps: &Value) -> Result<(), ApiError> {
    let list = steps
        .as_array()
        .ok_or_else(|| ApiError::bad_request("steps must be a list"))?;
    if list.is_empty() {
        return Err(ApiError::bad_request("a command needs at least one step"));
    }
    if list.len() > MAX_STEPS {
        return Err(ApiError::bad_request(&format!(
            "a command holds at most {MAX_STEPS} steps; chain longer sequences in the Automator"
        )));
    }
    for (i, step) in list.iter().enumerate() {
        let n = i + 1;
        let bad = |m: &str| ApiError::bad_request(&format!("step {n}: {m}"));
        match str_field(step, "kind") {
            "navigate" => {
                if !str_field(step, "target").starts_with('/') {
                    return Err(bad("pick the page to open"));
                }
            }
            "agent" => {
                if str_field(step, "prompt").is_empty() {
                    return Err(bad("write what to ask the agent"));
                }
            }
            "automator" => {
                if Uuid::parse_str(str_field(step, "workflow_id")).is_err() {
                    return Err(bad("pick the workflow to run"));
                }
            }
            "theme" => {
                if !matches!(str_field(step, "value"), "dark" | "light" | "toggle") {
                    return Err(bad("theme must be dark, light or toggle"));
                }
            }
            "privacy" => {
                if !matches!(str_field(step, "value"), "on" | "off" | "toggle") {
                    return Err(bad("privacy must be on, off or toggle"));
                }
            }
            "say" => {
                if str_field(step, "text").is_empty() {
                    return Err(bad("write what to say"));
                }
            }
            "" => return Err(bad("missing kind")),
            other => return Err(bad(&format!("unknown kind '{other}'"))),
        }
    }
    Ok(())
}

/// Clean the phrasings and refuse one another command already answers to: two commands on
/// the same words would make the spoken result depend on list order.
async fn validate_command(
    state: &AppState,
    input: &mut CommandInput,
    existing: Option<Uuid>,
) -> Result<(), ApiError> {
    input.phrase = input.phrase.trim().to_string();
    if norm(&input.phrase).is_empty() {
        return Err(ApiError::bad_request("say what the command answers to"));
    }
    let mut seen = vec![norm(&input.phrase)];
    let mut aliases = Vec::new();
    for a in &input.aliases {
        let a = a.trim();
        let n = norm(a);
        if n.is_empty() || seen.contains(&n) {
            continue;
        }
        seen.push(n);
        aliases.push(a.to_string());
    }
    input.aliases = aliases;
    validate_steps(&input.steps)?;
    for other in store::list_commands(&state.pool).await? {
        if Some(other.id) == existing {
            continue;
        }
        let theirs: Vec<String> =
            std::iter::once(&other.phrase).chain(other.aliases.iter()).map(|p| norm(p)).collect();
        if let Some(clash) = seen.iter().find(|p| theirs.contains(p)) {
            return Err(ApiError::conflict(&format!(
                "\"{clash}\" already triggers the command \"{}\"",
                other.phrase
            )));
        }
    }
    Ok(())
}

async fn list_commands(State(state): State<AppState>) -> Result<Json<Value>, ApiError> {
    Ok(Json(json!({ "commands": store::list_commands(&state.pool).await? })))
}

async fn add_command(
    State(state): State<AppState>,
    Json(mut input): Json<CommandInput>,
) -> Result<Json<Value>, ApiError> {
    validate_command(&state, &mut input, None).await?;
    let command = store::add_command(&state.pool, &input).await?;
    Ok(Json(json!({ "command": command })))
}

async fn update_command(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
    Json(mut input): Json<CommandInput>,
) -> Result<Json<Value>, ApiError> {
    validate_command(&state, &mut input, Some(id)).await?;
    let command = store::update_command(&state.pool, id, &input)
        .await?
        .ok_or_else(|| ApiError::not_found("voice command not found"))?;
    Ok(Json(json!({ "command": command })))
}

async fn delete_command(
    State(state): State<AppState>,
    Path(id): Path<Uuid>,
) -> Result<Json<Value>, ApiError> {
    if !store::delete_command(&state.pool, id).await? {
        return Err(ApiError::not_found("voice command not found"));
    }
    Ok(Json(json!({ "ok": true })))
}

#[derive(Deserialize)]
struct Reorder {
    ids: Vec<Uuid>,
}

async fn reorder_commands(
    State(state): State<AppState>,
    Json(input): Json<Reorder>,
) -> Result<Json<Value>, ApiError> {
    store::reorder_commands(&state.pool, &input.ids).await?;
    Ok(Json(json!({ "ok": true })))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phrases_compare_without_case_or_punctuation() {
        assert_eq!(norm("  Turtle, GO!  "), "turtle go");
        assert_eq!(norm("Ouvre   le journal"), "ouvre le journal");
    }

    #[test]
    fn steps_name_the_broken_one() {
        let ok = json!([{ "kind": "navigate", "target": "/journal" }, { "kind": "agent", "prompt": "hi" }]);
        assert!(validate_steps(&ok).is_ok());
        let bad = json!([{ "kind": "navigate", "target": "/journal" }, { "kind": "agent" }]);
        assert!(validate_steps(&bad).unwrap_err().message().starts_with("step 2"));
        assert!(validate_steps(&json!([])).is_err());
        assert!(validate_steps(&json!([{ "kind": "rm -rf" }])).is_err());
    }

    #[test]
    fn silence_is_a_valid_wav_header() {
        let w = silence_wav();
        assert_eq!(&w[0..4], b"RIFF");
        assert_eq!(&w[8..12], b"WAVE");
        assert_eq!(w.len(), 44 + 16_000);
    }
}
