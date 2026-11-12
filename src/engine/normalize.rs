use std::collections::HashMap;

use crate::core::Scope;
use url::Url;

use crate::engine::parse::ExtractedEndpoint;

#[derive(Debug, Clone)]
pub struct NormalizedEndpoint {
    pub url: String,
    pub normalized: String,
    pub path_template: String,
    pub params: Vec<String>,
    pub method: String,
    pub source_line: Option<u32>,
    pub context: Option<String>,
}

pub struct Normalizer {
    base: Url,
    scope: Scope,
}

impl Normalizer {
    pub fn new(base: Url, scope: Scope) -> Self {
        Self { base, scope }
    }

    pub fn normalize_raw(&self, raw: &str) -> Option<Url> {
        let raw = raw.trim();
        if raw.is_empty() {
            return None;
        }
        let url = if raw.starts_with("http://")
            || raw.starts_with("https://")
            || raw.starts_with("ws://")
            || raw.starts_with("wss://")
        {
            Url::parse(raw).ok()?
        } else {
            self.base.join(raw).ok()?
        };
        if !matches!(url.scheme(), "http" | "https" | "ws" | "wss") {
            return None;
        }
        if !self.scope.is_allowed(&url) {
            return None;
        }
        Some(url)
    }

    pub fn normalize_endpoint(
        &self,
        raw: &ExtractedEndpoint,
        _source_asset: Option<String>,
    ) -> Option<NormalizedEndpoint> {
        let url = self.normalize_raw(&raw.url)?;
        let (normalized, path_template, params) = normalize_url(&url);
        if is_noise(&normalized) {
            return None;
        }
        Some(NormalizedEndpoint {
            url: url.to_string(),
            normalized,
            path_template,
            params,
            method: raw
                .method
                .clone()
                .unwrap_or_else(|| "GET".into())
                .to_ascii_uppercase(),
            source_line: raw.source_line,
            context: raw.context.clone(),
        })
    }

    pub fn normalize_all(
        &self,
        endpoints: impl IntoIterator<Item = ExtractedEndpoint>,
        source_asset: Option<String>,
    ) -> Vec<NormalizedEndpoint> {
        let mut seen: HashMap<String, NormalizedEndpoint> = HashMap::new();
        for raw in endpoints {
            let Some(ep) = self.normalize_endpoint(&raw, source_asset.clone()) else {
                continue;
            };
            let key = format!("{} {}", ep.method, ep.normalized);
            match seen.get_mut(&key) {
                Some(existing) => {
                    if existing.context.is_none() {
                        existing.context = ep.context;
                    }
                    if existing.source_line.is_none() {
                        existing.source_line = ep.source_line;
                    }
                }
                None => {
                    seen.insert(key, ep);
                }
            }
        }
        let mut out: Vec<_> = seen.into_values().collect();
        out.sort_by(|a, b| {
            a.normalized
                .cmp(&b.normalized)
                .then_with(|| a.method.cmp(&b.method))
        });
        out
    }
}

pub fn normalize_url(url: &Url) -> (String, String, Vec<String>) {
    let mut clean = url.clone();
    clean.set_fragment(None);
    let mut params = Vec::new();
    let mut pairs: Vec<(String, String)> = clean
        .query_pairs()
        .filter(|(k, _)| !is_tracking_param(k))
        .map(|(k, v)| (k.into_owned(), v.into_owned()))
        .collect();
    pairs.sort();
    params.extend(pairs.iter().map(|(k, _)| k.clone()));
    if pairs.is_empty() {
        clean.set_query(None);
    } else {
        let query = pairs
            .iter()
            .map(|(k, v)| format!("{}={}", urlencoding(k), urlencoding(v)))
            .collect::<Vec<_>>()
            .join("&");
        clean.set_query(Some(&query));
    }
    let mut path = clean.path().to_string();
    while path.contains("//") {
        path = path.replace("//", "/");
    }
    if path.len() > 1 && path.ends_with('/') {
        path.pop();
    }
    clean.set_path(&path);

    let segments: Vec<&str> = path.split('/').collect();
    let mut template_segments = Vec::new();
    for segment in segments {
        if segment.is_empty() {
            continue;
        }
        if is_dynamic_segment(segment) {
            let label = if segment.chars().all(|c| c.is_ascii_digit()) {
                "{id}"
            } else if is_uuid(segment) {
                "{uuid}"
            } else if is_hex(segment) {
                "{hex}"
            } else {
                "{param}"
            };
            template_segments.push(label.to_string());
            params.push(label.trim_matches(['{', '}']).to_string());
        } else {
            template_segments.push(segment.to_string());
        }
    }
    let path_template = format!("/{}", template_segments.join("/"));
    params.sort();
    params.dedup();
    (clean.to_string(), path_template, params)
}

fn is_tracking_param(key: &str) -> bool {
    let lower = key.to_ascii_lowercase();
    lower.starts_with("utm_")
        || matches!(
            lower.as_str(),
            "fbclid" | "gclid" | "dclid" | "msclkid" | "mc_cid" | "mc_eid" | "ref" | "_ga"
        )
}

fn is_dynamic_segment(segment: &str) -> bool {
    if segment.is_empty() {
        return false;
    }
    if segment.chars().all(|c| c.is_ascii_digit()) && !segment.is_empty() {
        return true;
    }
    if segment.contains("${")
        || segment.contains("%7B")
        || segment.contains("%7b")
        || segment.contains('{')
    {
        return true;
    }
    is_uuid(segment) || is_hex(segment)
}

fn is_uuid(segment: &str) -> bool {
    let parts: Vec<&str> = segment.split('-').collect();
    parts.len() == 5
        && parts
            .iter()
            .all(|p| p.chars().all(|c| c.is_ascii_hexdigit()) && !p.is_empty())
}

fn is_hex(segment: &str) -> bool {
    segment.len() >= 12 && segment.chars().all(|c| c.is_ascii_hexdigit())
}

fn is_noise(normalized: &str) -> bool {
    let lower = normalized.to_ascii_lowercase();
    let noise = [
        ".png", ".jpg", ".jpeg", ".gif", ".svg", ".webp", ".avif", ".ico", ".woff", ".woff2",
        ".ttf", ".otf", ".eot", ".mp4", ".webm", ".mp3", ".wav", ".ogg", ".flac",
    ];
    if noise.iter().any(|ext| lower.contains(ext)) {
        return true;
    }
    if lower.contains("data:") || lower.contains("javascript:") || lower.contains("mailto:") {
        return true;
    }
    false
}

fn urlencoding(value: &str) -> String {
    url::form_urlencoded::byte_serialize(value.as_bytes()).collect()
}

pub fn is_page_url(url: &Url) -> bool {
    let path = url.path().to_ascii_lowercase();
    !path.contains('.') || path.ends_with(".html") || path.ends_with(".htm")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clusters_dynamic_paths_and_drops_tracking() {
        let url = Url::parse("https://example.com/api/users/123?utm_source=x&page=2").unwrap();
        let (normalized, template, params) = normalize_url(&url);
        assert!(normalized.contains("page=2"));
        assert!(!normalized.contains("utm_source"));
        assert_eq!(template, "/api/users/{id}");
        assert!(params.contains(&"page".to_string()));
        assert!(params.contains(&"id".to_string()));
    }

    #[test]
    fn normalizes_template_literal_placeholders() {
        let url = Url::parse("https://example.com/api/users/$%7Bid%7D").unwrap();
        let (_, template, _) = normalize_url(&url);
        assert_eq!(template, "/api/users/{param}");
    }
}
