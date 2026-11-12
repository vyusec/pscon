pub mod css;
pub mod html;
pub mod js;
pub mod json;
pub mod sourcemap;
pub mod xml;

use crate::core::AssetKind;
use url::Url;

#[derive(Debug, Clone, Default)]
pub struct Extracted {
    pub endpoints: Vec<ExtractedEndpoint>,
    pub assets: Vec<ExtractedAsset>,
    pub inline_scripts: Vec<String>,
    pub graphql: Vec<String>,
    pub source_map_urls: Vec<String>,
    pub comments: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedEndpoint {
    pub url: String,
    pub method: Option<String>,
    pub source_line: Option<u32>,
    pub context: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ExtractedAsset {
    pub url: String,
    pub kind: AssetKind,
}

pub fn parse(kind: AssetKind, source: &str, base: &Url) -> Extracted {
    match kind {
        AssetKind::JavaScript => js::parse(source, base),
        AssetKind::Html => html::parse(source, base),
        AssetKind::Css => css::parse(source, base),
        AssetKind::Json => json::parse(source, base),
        AssetKind::Xml => xml::parse(source, base),
        AssetKind::SourceMap => sourcemap::parse(source, base),
        AssetKind::GraphQl | AssetKind::Text | AssetKind::Other => {
            let mut out = Extracted::default();
            scan_raw(source, base, &mut out);
            out
        }
        _ => Extracted::default(),
    }
}

pub fn scan_raw(source: &str, base: &Url, out: &mut Extracted) {
    use once_cell::sync::Lazy;
    use regex::Regex;

    static URL_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)(?:https?|wss?)://[a-z0-9][a-z0-9._~:/?#\[\]@!$&'()*+,;=%-]{2,}"#)
            .expect("url regex")
    });
    static PATH_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"["'`](\/(?:api|v[0-9]+|graphql|gql|rest|auth|admin|internal|upload|debug|swagger|openapi|service)[a-zA-Z0-9_?&=\/\-#\.]*)"#)
            .expect("path regex")
    });
    static ASSET_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)(?:https?://|/)[^\s"'`<>()]+\.(?:js|mjs|css|json|xml|map|wasm)(?:\?[^\s"'`<>()]*)?"#)
            .expect("asset regex")
    });
    static SOURCE_MAP_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)sourceMappingURL\s*=\s*([^\s*]+)"#).expect("source map regex")
    });

    for m in URL_RE.find_iter(source) {
        add_endpoint(out, trim_url_punctuation(m.as_str()), None, None);
    }
    for cap in PATH_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            add_endpoint(out, m.as_str(), None, line_of(source, m.start()));
        }
    }
    for m in ASSET_RE.find_iter(source) {
        let raw = m.as_str();
        let url = raw
            .trim_start_matches('"')
            .trim_start_matches('\'')
            .to_string();
        if let Ok(resolved) = base.join(&url) {
            out.assets.push(ExtractedAsset {
                url: resolved.to_string(),
                kind: crate::engine::classify::classify_url(&resolved),
            });
        }
    }
    for cap in SOURCE_MAP_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            out.source_map_urls.push(m.as_str().to_string());
        }
    }
}

pub fn trim_url_punctuation(value: &str) -> &str {
    value.trim_end_matches([')', ']', '}', ',', ';', '\'', '"', '`', '.'])
}

pub fn add_endpoint(out: &mut Extracted, raw: &str, method: Option<String>, line: Option<u32>) {
    let value = trim_url_punctuation(
        raw.trim()
            .trim_matches('"')
            .trim_matches('\'')
            .trim_matches('`'),
    );
    if value.len() < 2 {
        return;
    }
    let lower = value.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("data:")
        || lower.starts_with("mailto:")
        || lower.starts_with("blob:")
        || lower == "//"
        || lower.starts_with("/*")
        || lower.contains(".png")
        || lower.contains(".jpg")
        || lower.contains(".jpeg")
        || lower.contains(".gif")
        || lower.contains(".svg")
        || lower.contains(".woff")
        || lower.contains(".ttf")
        || lower.contains(".ico")
    {
        return;
    }
    if !(lower.starts_with('/')
        || lower.starts_with("http://")
        || lower.starts_with("https://")
        || lower.starts_with("ws://")
        || lower.starts_with("wss://")
        || lower.starts_with('?')
        || lower.starts_with('#'))
    {
        return;
    }
    out.endpoints.push(ExtractedEndpoint {
        url: value.to_string(),
        method,
        source_line: line,
        context: None,
    });
}

pub fn line_of(source: &str, offset: usize) -> Option<u32> {
    if offset > source.len() {
        return None;
    }
    Some(source[..offset].bytes().filter(|b| *b == b'\n').count() as u32 + 1)
}

pub fn context_of(source: &str, start: usize, end: usize) -> Option<String> {
    let start = start.saturating_sub(80);
    let end = (end + 80).min(source.len());
    if start >= end || !source.is_char_boundary(start) || !source.is_char_boundary(end) {
        return None;
    }
    Some(source[start..end].replace('\n', " ").trim().to_string())
}
