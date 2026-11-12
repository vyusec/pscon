use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use super::{Extracted, ExtractedAsset, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();

    static URL_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)url\(\s*(?:'([^']+)'|"([^"]+)"|([^'")\s]+))\s*\)"#).expect("css url")
    });
    static IMPORT_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)@import\s+(?:url\()?\s*['"]?([^'")\s;]+)"#).expect("css import")
    });
    static MAP_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)sourceMappingURL\s*=\s*([^\s*]+)"#).expect("css source map")
    });

    for cap in URL_RE.captures_iter(source) {
        if let Some(m) = cap.get(1).or_else(|| cap.get(2)).or_else(|| cap.get(3)) {
            let value = m.as_str();
            if value.starts_with("data:") {
                continue;
            }
            if let Ok(resolved) = base.join(value) {
                out.assets.push(ExtractedAsset {
                    url: resolved.to_string(),
                    kind: crate::engine::classify::classify_url(&resolved),
                });
            }
        }
    }
    for cap in IMPORT_RE.captures_iter(source) {
        if let Some(m) = cap.get(1)
            && let Ok(resolved) = base.join(m.as_str())
        {
            out.assets.push(ExtractedAsset {
                url: resolved.to_string(),
                kind: crate::core::AssetKind::Css,
            });
        }
    }
    for cap in MAP_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            out.source_map_urls.push(m.as_str().to_string());
        }
    }

    scan_raw(source, base, &mut out);
    out
}
