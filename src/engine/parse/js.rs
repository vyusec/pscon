use oxc_allocator::Allocator;
use oxc_ast::ast::{
    Argument, CallExpression, Expression, ImportExpression, NewExpression, StringLiteral,
    TaggedTemplateExpression, TemplateLiteral,
};
use oxc_ast_visit::{Visit, walk};
use oxc_parser::Parser;
use oxc_span::{SourceType, Span};
use regex::Regex;
use url::Url;

use super::{Extracted, add_endpoint, context_of, line_of, scan_raw};

pub fn parse(source: &str, base: &Url) -> Extracted {
    let mut out = Extracted::default();
    scan_raw(source, base, &mut out);

    let allocator = Allocator::default();
    let source_type = SourceType::mjs();
    let ret = Parser::new(&allocator, source, source_type).parse();
    let mut collector = Collector { source, base, out };
    collector.visit_program(&ret.program);
    collector.out
}

struct Collector<'a> {
    source: &'a str,
    base: &'a Url,
    out: Extracted,
}

impl<'a> Visit<'a> for Collector<'a> {
    fn visit_string_literal(&mut self, lit: &StringLiteral<'a>) {
        self.consider(lit.value.as_str().unwrap_or_default(), lit.span);
        walk::walk_string_literal(self, lit);
    }

    fn visit_template_literal(&mut self, lit: &TemplateLiteral<'a>) {
        let text = template_text(lit);
        self.consider(&text, lit.span);
        walk::walk_template_literal(self, lit);
    }

    fn visit_call_expression(&mut self, expr: &CallExpression<'a>) {
        let callee = callee_name(&expr.callee);
        let raw = self.slice(expr.span);
        let method = raw.as_deref().and_then(extract_method);
        if is_network_callee(callee.as_deref()) {
            for arg in &expr.arguments {
                if let Some(value) = arg_text(arg) {
                    add_endpoint(
                        &mut self.out,
                        &value,
                        method.clone(),
                        line_of(self.source, expr.span.start as usize),
                    );
                    if let Some(last) = self.out.endpoints.last_mut() {
                        last.context = raw.clone();
                    }
                }
            }
        }
        walk::walk_call_expression(self, expr);
    }

    fn visit_new_expression(&mut self, expr: &NewExpression<'a>) {
        let callee = callee_name(&expr.callee);
        if is_network_callee(callee.as_deref()) {
            for arg in &expr.arguments {
                if let Some(value) = arg_text(arg) {
                    add_endpoint(
                        &mut self.out,
                        &value,
                        None,
                        line_of(self.source, expr.span.start as usize),
                    );
                }
            }
        }
        walk::walk_new_expression(self, expr);
    }

    fn visit_import_expression(&mut self, expr: &ImportExpression<'a>) {
        if let Some(value) = expression_text(&expr.source)
            && let Ok(resolved) = self.base.join(&value)
        {
            self.out.assets.push(super::ExtractedAsset {
                url: resolved.to_string(),
                kind: crate::engine::classify::classify_url(&resolved),
            });
        }
        walk::walk_import_expression(self, expr);
    }

    fn visit_tagged_template_expression(&mut self, expr: &TaggedTemplateExpression<'a>) {
        let tag = callee_name(&expr.tag);
        let text = template_text(&expr.quasi);
        if matches!(
            tag.as_deref(),
            Some("gql") | Some("graphql") | Some("gqlTag")
        ) {
            self.out.graphql.push(text.clone());
        }
        self.consider(&text, expr.quasi.span);
        walk::walk_tagged_template_expression(self, expr);
    }
}

impl Collector<'_> {
    fn consider(&mut self, value: &str, span: Span) {
        let line = line_of(self.source, span.start as usize);
        if looks_like_graphql(value) {
            self.out.graphql.push(value.to_string());
        }
        if looks_like_asset(value)
            && let Ok(resolved) = self.base.join(value)
        {
            self.out.assets.push(super::ExtractedAsset {
                url: resolved.to_string(),
                kind: crate::engine::classify::classify_url(&resolved),
            });
        }
        add_endpoint(&mut self.out, value, None, line);
        if let Some(last) = self.out.endpoints.last_mut()
            && last.source_line == line
        {
            last.context = context_of(self.source, span.start as usize, span.end as usize);
        }
    }

    fn slice(&self, span: Span) -> Option<String> {
        let start = span.start as usize;
        let end = span.end as usize;
        if start <= end && end <= self.source.len() {
            Some(self.source[start..end].to_string())
        } else {
            None
        }
    }
}

fn template_text(lit: &TemplateLiteral<'_>) -> String {
    let mut out = String::new();
    for (idx, quasi) in lit.quasis.iter().enumerate() {
        if let Some(cooked) = &quasi.value.cooked {
            out.push_str(cooked.as_str().unwrap_or_default());
        } else {
            out.push_str(quasi.value.raw.as_str());
        }
        if idx + 1 < lit.quasis.len() {
            out.push_str("${...}");
        }
    }
    out
}

fn arg_text(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::StringLiteral(s) => Some(s.value.as_str().unwrap_or_default().to_string()),
        Argument::TemplateLiteral(t) => Some(template_text(t)),
        _ => expression_text_from_argument(arg),
    }
}

fn expression_text_from_argument(arg: &Argument<'_>) -> Option<String> {
    match arg {
        Argument::Identifier(id) => Some(format!("${{{}}}", id.name)),
        _ => None,
    }
}

fn expression_text(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::StringLiteral(s) => Some(s.value.as_str().unwrap_or_default().to_string()),
        Expression::TemplateLiteral(t) => Some(template_text(t)),
        _ => None,
    }
}

fn callee_name(expr: &Expression<'_>) -> Option<String> {
    match expr {
        Expression::Identifier(id) => Some(id.name.to_string()),
        Expression::StaticMemberExpression(member) => {
            let object = callee_name(&member.object)?;
            Some(format!("{}.{}", object, member.property.name))
        }
        Expression::ComputedMemberExpression(member) => {
            let object = callee_name(&member.object)?;
            let prop = expression_text(&member.expression)?;
            Some(format!("{}.{}", object, prop))
        }
        _ => None,
    }
}

fn is_network_callee(name: Option<&str>) -> bool {
    let Some(name) = name else { return false };
    let name = name.to_ascii_lowercase();
    name == "fetch"
        || name == "axios"
        || name.starts_with("axios.")
        || name == "open"
        || name.ends_with(".open")
        || name == "sendbeacon"
        || name.ends_with(".sendbeacon")
        || name == "websocket"
        || name == "eventsource"
        || name == "$.ajax"
        || name == "jquery.ajax"
        || name == "superagent"
        || name.starts_with("http.")
}

fn extract_method(raw: &str) -> Option<String> {
    static METHOD_RE: once_cell::sync::Lazy<Regex> = once_cell::sync::Lazy::new(|| {
        Regex::new(r#"(?i)method\s*:\s*['"`]([a-z]+)['"`]"#).expect("method regex")
    });
    METHOD_RE
        .captures(raw)
        .and_then(|cap| cap.get(1).map(|m| m.as_str().to_ascii_uppercase()))
}

fn looks_like_graphql(value: &str) -> bool {
    let lower = value.trim_start().to_ascii_lowercase();
    lower.starts_with("query ")
        || lower.starts_with("mutation ")
        || lower.starts_with("subscription ")
        || lower.starts_with("fragment ")
}

fn looks_like_asset(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    (lower.starts_with('/') || lower.starts_with("http://") || lower.starts_with("https://"))
        && [".js", ".mjs", ".css", ".json", ".xml", ".map", ".wasm"]
            .iter()
            .any(|ext| lower.contains(ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_fetch_and_websocket() {
        let base = Url::parse("https://example.com/").unwrap();
        let source = r#"
            fetch('/api/v1/users', { method: 'POST' });
            const ws = new WebSocket('wss://example.com/socket');
            const gql = gql`query GetUsers { users { id } }`;
            import('/assets/chunk.js');
        "#;
        let out = parse(source, &base);
        assert!(out.endpoints.iter().any(|e| e.url == "/api/v1/users"));
        assert!(
            out.endpoints
                .iter()
                .any(|e| e.method.as_deref() == Some("POST"))
        );
        assert!(
            out.endpoints
                .iter()
                .any(|e| e.url.contains("wss://example.com/socket"))
        );
        assert!(!out.graphql.is_empty());
        assert!(out.assets.iter().any(|a| a.url.contains("chunk.js")));
    }
}
