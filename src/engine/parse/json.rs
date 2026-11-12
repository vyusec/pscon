use serde_json::Value;
use url::Url;

use super::{Extracted, ExtractedAsset, add_endpoint, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();
    match serde_json::from_str::<Value>(source) {
        Ok(value) => walk_value(&value, base, &mut out),
        Err(_) => scan_raw(source, base, &mut out),
    }
    scan_raw(source, base, &mut out);
    out
}

fn walk_value(value: &Value, base: &Url, out: &mut Extracted) {
    match value {
        Value::String(s) => {
            add_endpoint(out, s, None, None);
            if looks_like_asset(s)
                && let Ok(resolved) = base.join(s)
            {
                out.assets.push(ExtractedAsset {
                    url: resolved.to_string(),
                    kind: crate::engine::classify::classify_url(&resolved),
                });
            }
        }
        Value::Array(items) => {
            for item in items {
                walk_value(item, base, out);
            }
        }
        Value::Object(map) => {
            for (key, item) in map {
                let lower = key.to_ascii_lowercase();
                if (lower.contains("graphql")
                    || lower.contains("query")
                    || lower.contains("mutation"))
                    && let Some(s) = item.as_str()
                    && (s.contains("query") || s.contains("mutation") || s.contains("fragment"))
                {
                    out.graphql.push(s.to_string());
                }
                walk_value(item, base, out);
            }
        }
        _ => {}
    }
}

fn looks_like_asset(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with('/') || lower.starts_with("http://") || lower.starts_with("https://"))
        && [".js", ".mjs", ".css", ".json", ".xml", ".map", ".wasm"]
            .iter()
            .any(|ext| lower.contains(ext))
}
