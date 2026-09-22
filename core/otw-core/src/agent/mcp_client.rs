//! Outbound MCP client — connects the agent to REMOTE Streamable-HTTP MCP servers
//! (JSON-RPC over POST, rev 2025-06-18). Deliberately no stdio transport: a self-hosted
//! app must never spawn third-party processes, so only network servers are supported.
//!
//! Flow per server: `initialize` (captures the `Mcp-Session-Id` header when the server
//! is stateful) → `notifications/initialized` (best-effort) → `tools/list` / `tools/call`.
//! Responses may arrive as plain JSON or as an SSE stream (both allowed by the spec);
//! both are handled. All calls are wrapped in timeouts by the callers.

use anyhow::{anyhow, bail, Context};
use futures::StreamExt;
use serde_json::{json, Value};

use super::sse::SseDecoder;

const PROTOCOL_VERSION: &str = "2025-06-18";

pub struct McpClient {
    url: String,
    auth_header: String,
    auth_value: String,
    session: Option<String>,
}

impl McpClient {
    /// Initialize the connection; returns a ready client (session captured when present).
    pub async fn connect(url: &str, auth_header: &str, auth_value: &str) -> anyhow::Result<Self> {
        let mut client = Self {
            url: url.trim().to_string(),
            auth_header: auth_header.trim().to_string(),
            auth_value: auth_value.trim().to_string(),
            session: None,
        };
        if !client.url.starts_with("http://") && !client.url.starts_with("https://") {
            bail!("server URL must be http(s)");
        }
        let (resp_headers, result) = client
            .rpc_raw(
                "initialize",
                json!({
                    "protocolVersion": PROTOCOL_VERSION,
                    "capabilities": {},
                    "clientInfo": { "name": "opentraderworld-agent", "version": env!("CARGO_PKG_VERSION") }
                }),
                Some(1),
            )
            .await?;
        if let Some(sid) = resp_headers.get("mcp-session-id").and_then(|v| v.to_str().ok()) {
            client.session = Some(sid.to_string());
        }
        // Sanity: an MCP server answers initialize with a result carrying protocolVersion.
        if result.get("protocolVersion").is_none() && result.get("capabilities").is_none() {
            bail!("not an MCP server (unexpected initialize response)");
        }
        // Spec: notify initialized before normal operation. Best-effort (many stateless
        // servers 202/ignore it).
        let _ = client.notify("notifications/initialized").await;
        Ok(client)
    }

    /// `tools/list` → (name, description, input_schema) triples.
    pub async fn list_tools(&self) -> anyhow::Result<Vec<(String, String, Value)>> {
        let (_, result) = self.rpc_raw("tools/list", json!({}), Some(2)).await?;
        let tools = result
            .get("tools")
            .and_then(|t| t.as_array())
            .ok_or_else(|| anyhow!("tools/list returned no tools array"))?;
        Ok(tools
            .iter()
            .filter_map(|t| {
                let name = t.get("name")?.as_str()?.to_string();
                let desc = t.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let schema = t
                    .get("inputSchema")
                    .cloned()
                    .unwrap_or_else(|| json!({ "type": "object", "properties": {} }));
                Some((name, desc, schema))
            })
            .collect())
    }

    /// `tools/call` → (concatenated text content, is_error).
    pub async fn call_tool(&self, name: &str, args: &Value) -> anyhow::Result<(String, bool)> {
        let (_, result) = self
            .rpc_raw("tools/call", json!({ "name": name, "arguments": args }), Some(3))
            .await?;
        let is_error = result.get("isError").and_then(|e| e.as_bool()).unwrap_or(false);
        let text = result
            .get("content")
            .and_then(|c| c.as_array())
            .map(|items| {
                items
                    .iter()
                    .filter_map(|i| match i.get("type").and_then(|t| t.as_str()) {
                        Some("text") => i.get("text").and_then(|x| x.as_str()).map(String::from),
                        Some(other) => Some(format!("[{other} content omitted]")),
                        None => None,
                    })
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        Ok((text, is_error))
    }

    /// Fire a JSON-RPC notification (no id, no response expected).
    async fn notify(&self, method: &str) -> anyhow::Result<()> {
        self.request(&json!({ "jsonrpc": "2.0", "method": method })).await?;
        Ok(())
    }

    /// One JSON-RPC request → (response headers, `result` value). Errors on RPC `error`.
    async fn rpc_raw(
        &self,
        method: &str,
        params: Value,
        id: Option<u64>,
    ) -> anyhow::Result<(reqwest::header::HeaderMap, Value)> {
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let resp = self.request(&body).await?;
        let headers = resp.headers().clone();
        if !resp.status().is_success() {
            let status = resp.status().as_u16();
            let text = resp.text().await.unwrap_or_default();
            bail!("{}", super::friendly_http_error(status, &text));
        }

        let ct = headers
            .get(reqwest::header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string();
        let msg: Value = if ct.contains("text/event-stream") {
            // Stream frames until the response for our id (or the first response) shows up.
            let mut decoder = SseDecoder::new();
            let mut stream = resp.bytes_stream();
            let mut found: Option<Value> = None;
            'outer: while let Some(chunk) = stream.next().await {
                let bytes = chunk.context("reading mcp sse stream")?;
                for data in decoder.push(&bytes) {
                    let Ok(v) = serde_json::from_str::<Value>(&data) else { continue };
                    let matches_id =
                        id.is_some_and(|want| v.get("id").and_then(|i| i.as_u64()) == Some(want));
                    if matches_id || (v.get("result").is_some() || v.get("error").is_some()) {
                        found = Some(v);
                        break 'outer;
                    }
                }
            }
            found.ok_or_else(|| anyhow!("mcp server closed the stream without a response"))?
        } else {
            let text = resp.text().await.context("reading mcp response")?;
            if text.trim().is_empty() {
                // Notifications get 202/empty bodies.
                return Ok((headers, Value::Null));
            }
            serde_json::from_str(&text).context("parsing mcp response json")?
        };

        if let Some(err) = msg.get("error") {
            let m = err.get("message").and_then(|m| m.as_str()).unwrap_or("rpc error");
            bail!("mcp server error: {m}");
        }
        Ok((headers, msg.get("result").cloned().unwrap_or(Value::Null)))
    }

    /// Build + send the POST with protocol/auth/session headers.
    async fn request(&self, body: &Value) -> anyhow::Result<reqwest::Response> {
        let mut req = super::http()
            .post(&self.url)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("mcp-protocol-version", PROTOCOL_VERSION)
            .json(body);
        if !self.auth_value.is_empty() && !self.auth_header.is_empty() {
            req = req.header(&self.auth_header, &self.auth_value);
        }
        if let Some(sid) = &self.session {
            req = req.header("mcp-session-id", sid);
        }
        crate::rate::send(&super::host_of(&self.url), req)
            .await
            .context("sending mcp request")
    }
}
