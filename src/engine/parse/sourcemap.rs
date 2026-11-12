use sourcemap::SourceMap;
use url::Url;

use super::{Extracted, ExtractedAsset, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();
    let Ok(map) = SourceMap::from_slice(source.as_bytes()) else {
        scan_raw(source, base, &mut out);
        return out;
    };

    for src in map.sources() {
        let value = src.trim();
        if value.is_empty() {
            continue;
        }
        if let Ok(resolved) = base.join(value) {
            let kind = crate::engine::classify::classify_url(&resolved);
            out.assets.push(ExtractedAsset {
                url: resolved.to_string(),
                kind,
            });
        }
    }

    for content in map.source_contents().flatten() {
        if !content.trim().is_empty() {
            out.inline_scripts.push(content.to_string());
        }
    }

    scan_raw(source, base, &mut out);
    out
}
