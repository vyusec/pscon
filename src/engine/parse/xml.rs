use once_cell::sync::Lazy;
use regex::Regex;
use url::Url;

use super::{Extracted, ExtractedAsset, add_endpoint, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();

    static LOCATION_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)(?:location|address|endpoint|url|href)\s*=\s*["']([^"']+)["']"#)
            .expect("xml location")
    });
    static SOAP_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?is)<(?:soap|wsdl):[^>]+(?:location|address)\s*=\s*["']([^"']+)["']"#)
            .expect("soap location")
    });

    for cap in LOCATION_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            add_endpoint(
                &mut out,
                m.as_str(),
                None,
                super::line_of(source, m.start()),
            );
        }
    }
    for cap in SOAP_RE.captures_iter(source) {
        if let Some(m) = cap.get(1) {
            add_endpoint(
                &mut out,
                m.as_str(),
                None,
                super::line_of(source, m.start()),
            );
        }
    }

    static ASSET_RE: Lazy<Regex> = Lazy::new(|| {
        Regex::new(r#"(?i)(?:https?://|/)[^\s"'<>]+\.(?:js|mjs|css|json|xml|wsdl|xsd|map)(?:\?[^\s"'<>]*)?"#)
            .expect("xml asset")
    });
    for m in ASSET_RE.find_iter(source) {
        if let Ok(resolved) = base.join(m.as_str()) {
            out.assets.push(ExtractedAsset {
                url: resolved.to_string(),
                kind: crate::engine::classify::classify_url(&resolved),
            });
        }
    }

    scan_raw(source, base, &mut out);
    out
}
