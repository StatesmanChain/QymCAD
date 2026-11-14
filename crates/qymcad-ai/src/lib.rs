//! A chat-completions client returning bounded CAD recipes, never executable code.
use qymcad_core::ai::Plan;
use serde::{Deserialize, Serialize};
use std::time::Duration;
const MAX_REPLY: usize = 256 * 1024;
#[derive(Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Connection {
    pub endpoint: String,
    pub model: String,
    #[serde(skip)]
    pub key: String,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Proposal {
    pub message: String,
    pub plan: Option<Plan>,
}
#[derive(Clone)]
pub struct Exchange {
    pub prompt: String,
    pub reply: Proposal,
}
const INSTRUCTIONS: &str = include_str!("instructions.txt");
pub fn parse(text: &str) -> Result<Proposal, &'static str> {
    if text.len() > MAX_REPLY {
        return Err("ai-error-reply");
    }
    let text = text.trim();
    let text = if let Some(t) = text.strip_prefix("```json").or_else(|| text.strip_prefix("```")) {
        t.trim().strip_suffix("```").ok_or("ai-error-reply")?.trim()
    } else {
        text
    };
    let proposal: Proposal = serde_json::from_str(text).map_err(|_| "ai-error-reply")?;
    if proposal.message.len() > 8000 {
        return Err("ai-error-reply");
    }
    if let Some(plan) = &proposal.plan {
        let _ = plan.build()?;
    }
    Ok(proposal)
}
fn endpoint(text: &str) -> Result<url::Url, &'static str> {
    let u = url::Url::parse(text.trim()).map_err(|_| "ai-error-settings")?;
    if !matches!(u.scheme(), "https" | "http") || u.host_str().is_none() || !u.username().is_empty() || u.password().is_some() || u.fragment().is_some() || u.query().is_some() {
        return Err("ai-error-settings");
    }
    Ok(u)
}
/// Only this conversation is transmitted; the open document is never serialized.
pub fn ask(connection: &Connection, prompt: &str, previous: &[Exchange]) -> Result<Proposal, &'static str> {
    let endpoint = endpoint(&connection.endpoint)?;
    if connection.model.trim().is_empty() || connection.model.len() > 200 || connection.key.len() > 4096 {
        return Err("ai-error-settings");
    }
    if prompt.trim().is_empty() || prompt.len() > 12000 || previous.len() > 6 {
        return Err("ai-error-prompt");
    }
    let mut messages = vec![serde_json::json!({"role":"system","content":INSTRUCTIONS})];
    for e in previous {
        messages.push(serde_json::json!({"role":"user","content":e.prompt}));
        messages.push(serde_json::json!({"role":"assistant","content":serde_json::to_string(&e.reply).map_err(|_|"ai-error-reply")?}));
    }
    messages.push(serde_json::json!({"role":"user","content": user_message_content(prompt}));
    let bytes = serde_json::to_vec(&serde_json::json!({"model":connection.model.trim(),"messages":messages,"stream":false})).map_err(|_| "ai-error-reply")?;
    let agent: ureq::Agent = ureq::Agent::config_builder().timeout_global(Some(Duration::from_secs(120))).max_redirects(0).build().into();
    let mut request = agent.post(endpoint.as_str()).header("Content-Type", "application/json");
    if !connection.key.is_empty() {
        request = request.header("Authorization", format!("Bearer {}", connection.key));
    }
    let mut response = request.send(bytes).map_err(|e| match e {
        ureq::Error::StatusCode(401 | 403) => "ai-error-auth",
        ureq::Error::StatusCode(429) => "ai-error-quota",
        ureq::Error::StatusCode(_) => "ai-error-server",
        _ => "ai-error-network",
    })?;
    let text = response.body_mut().with_config().limit(MAX_REPLY as u64).read_to_string().map_err(|_| "ai-error-reply")?;
    parse_envelope(&text)
}
fn parse_envelope(text: &str) -> Result<Proposal, &'static str> {
    let e: serde_json::Value = serde_json::from_str(text).map_err(|_| "ai-error-reply")?;
    let c = e.get("choices").and_then(|v| v.get(0)).ok_or("ai-error-reply")?;
    if c.get("finish_reason").and_then(|v| v.as_str()).is_some_and(|s| s != "stop") {
        return Err("ai-error-reply");
    }
    parse(c.pointer("/message/content").and_then(|v| v.as_str()).ok_or("ai-error-reply")?)
}
#[cfg(test)]
mod tests;
fn base64_encode(data: &[u8]) -> String {
    const CHARSET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    let mut i = 0;
    while i < data.len() {
        let b0 = data[i] as u32;
        let b1 = if i + 1 < data.len() { data[i + 1] as u32 } else { 0 };
        let b2 = if i + 2 < data.len() { data[i + 2] as u32 } else { 0 };
        let triple = (b0 << 16) | (b1 << 8) | b2;
        out.push(CHARSET[((triple >> 18) & 0x3F) as usize] as char);
        out.push(CHARSET[((triple >> 12) & 0x3F) as usize] as char);
        out.push(if i + 1 < data.len() { CHARSET[((triple >> 6) & 0x3F) as usize] as char } else { '=' });
        out.push(if i + 2 < data.len() { CHARSET[(triple & 0x3F) as usize] as char } else { '=' });
        i += 3;
    }
    out
}
fn user_message_content(prompt: &str) -> serde_json::Value {
    let mut images = Vec::new();
    let mut seen = std::collections::HashSet::new();
    let mut check = |s: &str| {
        let cleaned = s.trim().trim_matches(|c| c == '"' || c == '\'');
        let path = std::path::Path::new(cleaned);
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { return };
        let mime = match ext.to_ascii_lowercase().as_str() {
            "png" => "image/png",
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            _ => return,
        };
        if path.is_file() && seen.insert(cleaned.to_owned()) {
            if let Ok(bytes) = std::fs::read(path) {
                let data_url = format!("data:{mime};base64,{}", base64_encode(&bytes));
                images.push(serde_json::json!({
                    "type": "image_url",
                    "image_url": { "url": data_url }
                }));
            }
        }
    };
    for line in prompt.lines() {
        check(line);
        for word in line.split_whitespace() {
            check(word);
        }
    }
    if images.is_empty() {
        serde_json::json!(prompt)
    } else {
        let mut blocks = vec![serde_json::json!({ "type": "text", "text": prompt })];
        blocks.extend(images);
        serde_json::Value::Array(blocks)
    }
}
