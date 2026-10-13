use std::collections::HashMap;

use serde_json::{Map, Value, json};

use crate::radius::error::RadiusError;

/// Decoded RADIUS request attributes from the `rlm_rest` JSON body.
///
/// Accepts the three shapes `rlm_rest` may emit: a bare string, an array of
/// strings/numbers, or `{ "value": [ ... ] }`.
#[derive(Debug, Default, Clone)]
pub struct RadiusRequest {
    attributes: HashMap<String, Vec<String>>,
}

impl RadiusRequest {
    pub fn from_json(value: &Value) -> Result<Self, RadiusError> {
        let object = value.as_object().ok_or_else(|| RadiusError::BadRequest {
            message: "expected a JSON object of attributes".to_string(),
        })?;

        let mut attributes = HashMap::new();
        for (name, raw) in object {
            let values = extract_values(raw);
            if !values.is_empty() {
                attributes.insert(name.clone(), values);
            }
        }
        Ok(Self { attributes })
    }

    pub fn get(&self, name: &str) -> Option<&str> {
        self.attributes
            .get(name)
            .and_then(|values| values.first())
            .map(String::as_str)
    }

    pub fn get_u64(&self, name: &str) -> Option<u64> {
        self.get(name).and_then(|value| value.trim().parse().ok())
    }

    pub fn get_all(&self, name: &str) -> &[String] {
        self.attributes.get(name).map(Vec::as_slice).unwrap_or(&[])
    }
}

fn extract_values(raw: &Value) -> Vec<String> {
    match raw {
        Value::Null => Vec::new(),
        Value::String(text) => vec![text.clone()],
        Value::Number(number) => vec![number.to_string()],
        Value::Bool(flag) => vec![flag.to_string()],
        Value::Array(items) => items.iter().flat_map(extract_values).collect(),
        Value::Object(object) => match object.get("value") {
            Some(inner) => extract_values(inner),
            None => Vec::new(),
        },
    }
}

/// Response attributes in the `rlm_rest` JSON shape:
/// `{ "reply:X": { "value": ["v"], "op": ":=" }, ... }`.
#[derive(Debug, Default)]
pub struct RadiusResponse {
    attributes: Map<String, Value>,
}

impl RadiusResponse {
    pub fn new() -> Self {
        Self::default()
    }

    /// Assign a single value (`:=`).
    pub fn set(&mut self, attribute: &str, value: impl Into<String>) {
        self.attributes.insert(
            attribute.to_string(),
            json!({ "value": [value.into()], "op": ":=" }),
        );
    }

    /// Append a value (`+=`).
    pub fn add(&mut self, attribute: &str, value: impl Into<String>) {
        self.attributes.insert(
            attribute.to_string(),
            json!({ "value": [value.into()], "op": "+=" }),
        );
    }

    pub fn is_empty(&self) -> bool {
        self.attributes.is_empty()
    }

    pub fn into_json(self) -> Value {
        Value::Object(self.attributes)
    }
}

/// Format a download/upload rate pair as a RouterOS `Mikrotik-Rate-Limit`
/// value (`rx-rate[/tx-rate]`, with rx = upload, tx = download).
pub fn rate_limit(upload_bps: Option<u64>, download_bps: Option<u64>) -> Option<String> {
    match (upload_bps, download_bps) {
        (None, None) => None,
        (Some(up), None) => Some(format!("{0}/{0}", format_bps(up))),
        (None, Some(down)) => Some(format!("{0}/{0}", format_bps(down))),
        (Some(up), Some(down)) if up == down => Some(format!("{0}/{0}", format_bps(up))),
        (Some(up), Some(down)) => Some(format!("{}/{}", format_bps(up), format_bps(down))),
    }
}

fn format_bps(bps: u64) -> String {
    if bps >= 1_000_000 && bps.is_multiple_of(1_000_000) {
        format!("{}M", bps / 1_000_000)
    } else if bps >= 1_000 && bps.is_multiple_of(1_000) {
        format!("{}k", bps / 1_000)
    } else {
        bps.to_string()
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::{RadiusRequest, RadiusResponse, rate_limit};

    #[test]
    fn parses_all_rlm_rest_shapes() {
        let request = RadiusRequest::from_json(&json!({
            "User-Name": "bob",
            "NAS-IP-Address": { "value": ["10.0.0.1"] },
            "Acct-Session-Time": { "value": [42] },
            "Framed-IP-Address": ["10.5.0.2"]
        }))
        .expect("parse");

        assert_eq!(request.get("User-Name"), Some("bob"));
        assert_eq!(request.get("NAS-IP-Address"), Some("10.0.0.1"));
        assert_eq!(request.get_u64("Acct-Session-Time"), Some(42));
        assert_eq!(request.get("Framed-IP-Address"), Some("10.5.0.2"));
    }

    #[test]
    fn builds_response_attributes() {
        let mut response = RadiusResponse::new();
        response.set("control:Auth-Type", "rest");
        response.set("reply:Mikrotik-Rate-Limit", "5M/2M");
        let value = response.into_json();

        assert_eq!(value["control:Auth-Type"]["value"][0], "rest");
        assert_eq!(value["control:Auth-Type"]["op"], ":=");
        assert_eq!(value["reply:Mikrotik-Rate-Limit"]["value"][0], "5M/2M");
    }

    #[test]
    fn formats_rate_limits() {
        assert_eq!(rate_limit(None, None), None);
        assert_eq!(
            rate_limit(Some(5_000_000), Some(2_000_000)).as_deref(),
            Some("5M/2M")
        );
        assert_eq!(
            rate_limit(Some(1_000_000), Some(1_000_000)).as_deref(),
            Some("1M/1M")
        );
        assert_eq!(
            rate_limit(None, Some(512_000)).as_deref(),
            Some("512k/512k")
        );
        assert_eq!(rate_limit(Some(1500), None).as_deref(), Some("1500/1500"));
    }
}
