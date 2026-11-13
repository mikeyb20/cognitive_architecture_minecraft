//! One JSON POST, shared by both wire formats (spec §23).

use std::time::Duration;

use serde_json::Value;

use crate::LlmError;

/// Longest a call may take before it counts as failed.
const TIMEOUT: Duration = Duration::from_secs(120);

pub(crate) fn agent() -> ureq::Agent {
    let config = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .http_status_as_error(false)
        .build();
    ureq::Agent::new_with_config(config)
}

/// POST `body` to `url` and parse the answer, mapping every failure to an
/// [`LlmError`].
pub(crate) fn post(
    agent: &ureq::Agent,
    url: &str,
    headers: &[(&str, &str)],
    body: &Value,
) -> Result<Value, LlmError> {
    let mut req = agent.post(url).header("content-type", "application/json");
    for (k, v) in headers {
        req = req.header(*k, *v);
    }
    let mut resp = req
        .send(body.to_string())
        .map_err(|e| LlmError::Transport(e.to_string()))?;
    let status = resp.status().as_u16();
    let text = resp
        .body_mut()
        .read_to_string()
        .map_err(|e| LlmError::Transport(e.to_string()))?;
    if status != 200 {
        return Err(LlmError::Status(status, text));
    }
    serde_json::from_str(&text).map_err(|e| LlmError::Parse(e.to_string()))
}

/// A whole-number field of a JSON object, or a parse error naming it.
pub(crate) fn count(v: &Value, field: &str) -> Result<u32, LlmError> {
    v.get(field)
        .and_then(Value::as_u64)
        .map(|n| n as u32)
        .ok_or_else(|| LlmError::Parse(format!("missing {field}")))
}
