use std::net::IpAddr;
use std::path::Path;

use ipnet::IpNet;
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ScopeError {
    #[error("out of scope: {0}")]
    OutOfScope(String),
    #[error("invalid scope pattern: {0}")]
    InvalidPattern(String),
}

#[derive(Debug, Clone)]
enum Rule {
    Any,
    Host(String),
    Suffix(String),
    Ip(IpNet),
}

#[derive(Debug, Clone)]
pub struct Scope {
    allow: Vec<Rule>,
    deny: Vec<Rule>,
}

impl Scope {
    pub fn allow_all() -> Self {
        Self {
            allow: vec![Rule::Any],
            deny: Vec::new(),
        }
    }

    pub fn from_target(target: &Url) -> Self {
        let mut allow = Vec::new();
        if let Some(host) = target.host_str() {
            allow.push(Rule::Host(host.to_ascii_lowercase()));
            if let Ok(ip) = host.parse::<IpAddr>() {
                if let Ok(net) = IpNet::new(ip, if ip.is_ipv4() { 32 } else { 128 }) {
                    allow.push(Rule::Ip(net));
                }
            } else {
                allow.push(Rule::Suffix(format!(".{}", host.to_ascii_lowercase())));
            }
        }
        Self {
            allow,
            deny: Vec::new(),
        }
    }

    pub fn from_file(path: &Path, target: &Url) -> Result<Self, ScopeError> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| ScopeError::InvalidPattern(format!("{}: {e}", path.display())))?;
        let mut scope = Self::from_target(target);
        scope.allow.clear();
        for raw in text.lines() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let deny = line.starts_with('!');
            let pattern = line.trim_start_matches('!').trim();
            let rule = parse_rule(pattern)?;
            if deny {
                scope.deny.push(rule);
            } else {
                scope.allow.push(rule);
            }
        }
        if scope.allow.is_empty() {
            let default = Self::from_target(target);
            scope.allow = default.allow;
        }
        Ok(scope)
    }

    pub fn is_allowed(&self, url: &Url) -> bool {
        let host = match url.host_str() {
            Some(h) => h.to_ascii_lowercase(),
            None => return false,
        };
        if self.deny.iter().any(|rule| rule.matches(&host)) {
            return false;
        }
        self.allow.iter().any(|rule| rule.matches(&host))
    }

    pub fn check(&self, url: &Url) -> Result<(), ScopeError> {
        if self.is_allowed(url) {
            Ok(())
        } else {
            Err(ScopeError::OutOfScope(url.to_string()))
        }
    }
}

impl Rule {
    fn matches(&self, host: &str) -> bool {
        match self {
            Rule::Any => true,
            Rule::Host(h) => h == host,
            Rule::Suffix(suffix) => {
                host.ends_with(suffix) || host == suffix.trim_start_matches('.')
            }
            Rule::Ip(net) => host
                .parse::<IpAddr>()
                .map(|ip| net.contains(&ip))
                .unwrap_or(false),
        }
    }
}

fn parse_rule(pattern: &str) -> Result<Rule, ScopeError> {
    let pattern = pattern.trim();
    if pattern == "*" || pattern == "0.0.0.0/0" || pattern == "::/0" {
        return Ok(Rule::Any);
    }
    if pattern.contains('/') {
        if let Ok(net) = pattern.parse::<IpNet>() {
            return Ok(Rule::Ip(net));
        }
        return Err(ScopeError::InvalidPattern(pattern.to_string()));
    }
    if let Some(rest) = pattern.strip_prefix("*.") {
        return Ok(Rule::Suffix(format!(".{}", rest.to_ascii_lowercase())));
    }
    if let Ok(ip) = pattern.parse::<IpAddr>() {
        let bits = if ip.is_ipv4() { 32 } else { 128 };
        return IpNet::new(ip, bits)
            .map(Rule::Ip)
            .map_err(|_| ScopeError::InvalidPattern(pattern.to_string()));
    }
    Ok(Rule::Host(pattern.to_ascii_lowercase()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wildcard_and_deny_rules() {
        let path = std::env::temp_dir().join(format!("pscon-scope-{}.txt", uuid::Uuid::new_v4()));
        std::fs::write(&path, "*.example.com\n!internal.example.com\n").unwrap();
        let target = Url::parse("https://example.com/").unwrap();
        let scope = Scope::from_file(&path, &target).unwrap();
        assert!(scope.is_allowed(&Url::parse("https://app.example.com/").unwrap()));
        assert!(!scope.is_allowed(&Url::parse("https://internal.example.com/").unwrap()));
        assert!(!scope.is_allowed(&Url::parse("https://other.test/").unwrap()));
        let _ = std::fs::remove_file(path);
    }
}
