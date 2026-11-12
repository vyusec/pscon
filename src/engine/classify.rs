use crate::core::AssetKind;
use url::Url;

pub fn classify_url(url: &Url) -> AssetKind {
    let path = url.path().to_ascii_lowercase();
    let ext = path.rsplit('.').next().unwrap_or("");
    match ext {
        "html" | "htm" | "xhtml" => AssetKind::Html,
        "js" | "mjs" | "cjs" | "jsx" | "ts" | "tsx" => AssetKind::JavaScript,
        "css" => AssetKind::Css,
        "json" | "jsonl" | "ndjson" => AssetKind::Json,
        "xml" | "wsdl" | "xsd" | "soap" => AssetKind::Xml,
        "graphql" | "gql" => AssetKind::GraphQl,
        "map" => AssetKind::SourceMap,
        "wasm" => AssetKind::Wasm,
        "txt" | "md" | "csv" | "yaml" | "yml" | "toml" | "ini" | "conf" | "config" => {
            AssetKind::Text
        }
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "avif" | "svg" | "ico" | "bmp" => {
            AssetKind::Image
        }
        "woff" | "woff2" | "ttf" | "otf" | "eot" => AssetKind::Font,
        "mp4" | "webm" | "mp3" | "wav" | "ogg" | "flac" | "m4a" => AssetKind::Media,
        _ => AssetKind::Other,
    }
}

pub fn classify_content_type(content_type: Option<&str>, url: Option<&Url>) -> AssetKind {
    let ct = content_type.unwrap_or("").to_ascii_lowercase();
    if ct.contains("javascript") || ct.contains("ecmascript") {
        return AssetKind::JavaScript;
    }
    if ct.contains("text/css") {
        return AssetKind::Css;
    }
    if ct.contains("text/html") {
        return AssetKind::Html;
    }
    if ct.contains("application/json") || ct.contains("+json") {
        return AssetKind::Json;
    }
    if ct.contains("xml") || ct.contains("soap") {
        return AssetKind::Xml;
    }
    if ct.contains("graphql") {
        return AssetKind::GraphQl;
    }
    if ct.contains("sourcemap") {
        return AssetKind::SourceMap;
    }
    if ct.contains("wasm") {
        return AssetKind::Wasm;
    }
    if ct.starts_with("image/") {
        return AssetKind::Image;
    }
    if ct.starts_with("font/") || ct.contains("font") {
        return AssetKind::Font;
    }
    if ct.starts_with("audio/") || ct.starts_with("video/") {
        return AssetKind::Media;
    }
    if ct.starts_with("text/") {
        return AssetKind::Text;
    }
    url.map(classify_url).unwrap_or(AssetKind::Other)
}

pub fn is_interesting(kind: AssetKind) -> bool {
    matches!(
        kind,
        AssetKind::JavaScript
            | AssetKind::Css
            | AssetKind::Json
            | AssetKind::Xml
            | AssetKind::GraphQl
            | AssetKind::SourceMap
            | AssetKind::Text
    )
}

pub fn is_static(kind: AssetKind) -> bool {
    matches!(
        kind,
        AssetKind::Image | AssetKind::Font | AssetKind::Media | AssetKind::Wasm
    )
}
