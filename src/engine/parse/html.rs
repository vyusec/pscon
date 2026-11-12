use once_cell::sync::Lazy;
use regex::Regex;
use scraper::{Html, Selector};
use url::Url;

use super::{Extracted, ExtractedAsset, ExtractedEndpoint, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();
    let doc = Html::parse_document(source);

    for (selector, attr, kind) in [
        ("a[href]", "href", "page"),
        ("area[href]", "href", "page"),
        ("iframe[src]", "src", "page"),
        ("frame[src]", "src", "page"),
        ("script[src]", "src", "script"),
        ("link[href]", "href", "link"),
        ("form[action]", "action", "form"),
        ("img[src]", "src", "image"),
        ("source[src]", "src", "media"),
        ("video[src]", "src", "media"),
        ("audio[src]", "src", "media"),
        ("object[data]", "data", "object"),
        ("embed[src]", "src", "object"),
    ] {
        let Ok(sel) = Selector::parse(selector) else {
            continue;
        };
        for el in doc.select(&sel) {
            let Some(value) = el.value().attr(attr) else {
                continue;
            };
            if value.is_empty() || value.starts_with('#') {
                continue;
            }
            let Ok(resolved) = base.join(value) else {
                continue;
            };
            match kind {
                "script" => out.assets.push(ExtractedAsset {
                    url: resolved.to_string(),
                    kind: crate::core::AssetKind::JavaScript,
                }),
                "link" => {
                    let rel = el.value().attr("rel").unwrap_or("").to_ascii_lowercase();
                    let kind = if rel.contains("stylesheet") {
                        crate::core::AssetKind::Css
                    } else if rel.contains("manifest") {
                        crate::core::AssetKind::Json
                    } else {
                        crate::engine::classify::classify_url(&resolved)
                    };
                    out.assets.push(ExtractedAsset {
                        url: resolved.to_string(),
                        kind,
                    });
                }
                "image" | "media" | "object" => out.assets.push(ExtractedAsset {
                    url: resolved.to_string(),
                    kind: crate::engine::classify::classify_url(&resolved),
                }),
                _ => out.endpoints.push(ExtractedEndpoint {
                    url: resolved.to_string(),
                    method: if kind == "form" {
                        el.value().attr("method").map(|m| m.to_ascii_uppercase())
                    } else {
                        Some("GET".into())
                    },
                    source_line: None,
                    context: Some(format!("{selector} {attr}")),
                }),
            }
        }
    }

    let Ok(script_sel) = Selector::parse("script:not([src])") else {
        return out;
    };
    for el in doc.select(&script_sel) {
        let text = el.text().collect::<String>();
        if !text.trim().is_empty() {
            out.inline_scripts.push(text);
        }
    }

    let Ok(json_sel) = Selector::parse("script[type='application/ld+json']") else {
        return out;
    };
    for el in doc.select(&json_sel) {
        let text = el.text().collect::<String>();
        if !text.trim().is_empty() {
            out.inline_scripts.push(text);
        }
    }

    static COMMENT_RE: Lazy<Regex> =
        Lazy::new(|| Regex::new(r"(?s)<!--(.*?)-->").expect("comment regex"));
    for cap in COMMENT_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            let text = m.as_str().trim();
            if !text.is_empty() {
                out.comments.push(text.to_string());
            }
        }
    }

    static DATA_URL_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)data-(?:url|endpoint|api|src|href)\s*=\s*["']([^"']+)["']"#)
            .expect("data url regex")
    });
    for cap in DATA_URL_RE.captures_iter(source) {
        if let Some(m) = cap.get(1)
            && let Ok(resolved) = base.join(m.as_str())
        {
            out.endpoints.push(ExtractedEndpoint {
                url: resolved.to_string(),
                method: Some("GET".into()),
                source_line: super::line_of(source, m.start()),
                context: Some("data-* attribute".into()),
            });
        }
    }

    scan_raw(source, base, &mut out);
    out
}
