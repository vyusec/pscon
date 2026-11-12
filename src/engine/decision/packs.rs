use super::answer::DecisionResponse;
use super::question::{QuestionSet, choice, noul, question_set, score};

pub fn endpoint_triage() -> QuestionSet {
    question_set(vec![
        (
            "role",
            choice(
                "What kind of resource is this URL?",
                &[
                    ("api", "REST or JSON API endpoint"),
                    ("page", "HTML page or client-side route"),
                    ("static", "static asset such as JS, CSS, image, or font"),
                    ("auth", "login, logout, token, OAuth, or session endpoint"),
                    (
                        "admin",
                        "administrative, internal, or privileged functionality",
                    ),
                    ("upload", "file upload, import, or attachment endpoint"),
                    ("graphql", "GraphQL endpoint or operation"),
                    ("websocket", "WebSocket or server-sent event endpoint"),
                    (
                        "config",
                        "configuration, manifest, environment, or metadata file",
                    ),
                    ("docs", "documentation, Swagger, OpenAPI, or API explorer"),
                    (
                        "telemetry",
                        "analytics, tracking, logging, or monitoring endpoint",
                    ),
                    ("noise", "library, polyfill, or irrelevant URL"),
                ],
            ),
        ),
        (
            "sensitivity",
            score(
                "How sensitive would this resource be if it were accessible?",
                &[
                    "public and non-sensitive",
                    "internal but low impact",
                    "sensitive business data or user data",
                    "privileged functionality or secrets",
                    "critical control plane or authentication bypass",
                ],
            ),
        ),
        (
            "auth_required",
            noul("Is this resource likely to require authentication or authorization?"),
        ),
        (
            "verify_safe",
            noul("Would a GET or HEAD request to this resource be safe and non-destructive?"),
        ),
        (
            "priority",
            score(
                "How much should a bug hunter prioritize reviewing this resource?",
                &["ignore", "low", "medium", "high", "critical"],
            ),
        ),
    ])
}

pub fn secret_triage() -> QuestionSet {
    question_set(vec![
        (
            "is_real",
            noul(
                "Does this look like a real credential rather than a placeholder, test value, or documentation example?",
            ),
        ),
        (
            "secret_class",
            choice(
                "What kind of credential is this?",
                &[
                    ("cloud", "AWS, GCP, Azure, or other cloud credential"),
                    (
                        "source_control",
                        "GitHub, GitLab, Bitbucket, or source-control token",
                    ),
                    ("payment", "Stripe, PayPal, or payment credential"),
                    (
                        "communication",
                        "Slack, Discord, Teams, or messaging credential",
                    ),
                    ("ai", "OpenAI, Anthropic, or other AI provider key"),
                    (
                        "database",
                        "database connection string or database password",
                    ),
                    ("private_key", "private key or certificate material"),
                    ("jwt", "JSON Web Token or session token"),
                    ("generic", "generic API key or secret"),
                    ("test_placeholder", "test, example, or placeholder value"),
                ],
            ),
        ),
        (
            "risk",
            score(
                "What is the potential impact if this credential is valid?",
                &["negligible", "low", "medium", "high", "critical"],
            ),
        ),
        (
            "should_validate",
            noul("Would validating this credential be safe, non-destructive, and appropriate?"),
        ),
    ])
}

pub fn asset_triage() -> QuestionSet {
    question_set(vec![
        (
            "asset_role",
            choice(
                "What is this JavaScript or text asset?",
                &[
                    ("app_bundle", "first-party application bundle"),
                    ("vendor_bundle", "third-party library or vendor bundle"),
                    ("chunk", "code-split application chunk"),
                    ("worker", "web worker or service worker"),
                    ("config", "configuration or manifest data"),
                    ("polyfill", "polyfill or compatibility shim"),
                    ("analytics", "analytics or tracking script"),
                    ("noise", "irrelevant or duplicate asset"),
                ],
            ),
        ),
        (
            "worth_deep_parse",
            noul(
                "Is this asset likely to contain first-party application endpoints or secrets worth deep parsing?",
            ),
        ),
        (
            "contains_auth_logic",
            noul(
                "Does this asset likely contain authentication, authorization, or token-handling logic?",
            ),
        ),
        (
            "contains_graphql",
            noul(
                "Does this asset likely contain GraphQL queries, mutations, or schema references?",
            ),
        ),
    ])
}

pub fn verification_triage() -> QuestionSet {
    question_set(vec![
        (
            "accessible",
            noul(
                "Does the response indicate the resource is accessible without additional authentication?",
            ),
        ),
        (
            "auth_required",
            noul("Does the response indicate authentication or authorization is required?"),
        ),
        (
            "interesting",
            noul("Is this response interesting enough for a bug hunter to investigate further?"),
        ),
        (
            "risk",
            score(
                "How risky would this accessible resource be?",
                &["informational", "low", "medium", "high", "critical"],
            ),
        ),
    ])
}

#[derive(Debug, Clone)]
pub struct EndpointTriage {
    pub role: String,
    pub sensitivity: f32,
    pub auth_required: Option<bool>,
    pub verify_safe: Option<bool>,
    pub priority: f32,
    pub confidence: f32,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct SecretTriage {
    pub is_real: Option<bool>,
    pub secret_class: String,
    pub risk: f32,
    pub should_validate: Option<bool>,
    pub confidence: f32,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct AssetTriage {
    pub role: String,
    pub worth_deep_parse: Option<bool>,
    pub contains_auth_logic: Option<bool>,
    pub contains_graphql: Option<bool>,
    pub confidence: f32,
    pub provider: String,
}

#[derive(Debug, Clone)]
pub struct VerificationTriage {
    pub accessible: Option<bool>,
    pub auth_required: Option<bool>,
    pub interesting: Option<bool>,
    pub risk: f32,
    pub confidence: f32,
    pub provider: String,
}

impl EndpointTriage {
    pub fn from_response(response: &DecisionResponse) -> Self {
        let role = response
            .answer("role")
            .and_then(|a| a.as_choice())
            .map(|(c, _)| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        let sensitivity = response
            .answer("sensitivity")
            .and_then(|a| a.as_score())
            .map(|(s, _)| (s / 4.0).clamp(0.0, 1.0))
            .unwrap_or(0.3);
        let auth_required = response
            .answer("auth_required")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let verify_safe = response
            .answer("verify_safe")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let priority = response
            .answer("priority")
            .and_then(|a| a.as_score())
            .map(|(s, _)| (s / 4.0).clamp(0.0, 1.0))
            .unwrap_or(0.3);
        Self {
            role,
            sensitivity,
            auth_required,
            verify_safe,
            priority,
            confidence: response.min_confidence(),
            provider: response.provider.clone(),
        }
    }
}

impl SecretTriage {
    pub fn from_response(response: &DecisionResponse) -> Self {
        let is_real = response
            .answer("is_real")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let secret_class = response
            .answer("secret_class")
            .and_then(|a| a.as_choice())
            .map(|(c, _)| c.to_string())
            .unwrap_or_else(|| "generic".into());
        let risk = response
            .answer("risk")
            .and_then(|a| a.as_score())
            .map(|(s, _)| (s / 4.0).clamp(0.0, 1.0))
            .unwrap_or(0.4);
        let should_validate = response
            .answer("should_validate")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        Self {
            is_real,
            secret_class,
            risk,
            should_validate,
            confidence: response.min_confidence(),
            provider: response.provider.clone(),
        }
    }
}

impl AssetTriage {
    pub fn from_response(response: &DecisionResponse) -> Self {
        let role = response
            .answer("asset_role")
            .and_then(|a| a.as_choice())
            .map(|(c, _)| c.to_string())
            .unwrap_or_else(|| "unknown".into());
        let worth_deep_parse = response
            .answer("worth_deep_parse")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let contains_auth_logic = response
            .answer("contains_auth_logic")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let contains_graphql = response
            .answer("contains_graphql")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        Self {
            role,
            worth_deep_parse,
            contains_auth_logic,
            contains_graphql,
            confidence: response.min_confidence(),
            provider: response.provider.clone(),
        }
    }
}

impl VerificationTriage {
    pub fn from_response(response: &DecisionResponse) -> Self {
        let accessible = response
            .answer("accessible")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let auth_required = response
            .answer("auth_required")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let interesting = response
            .answer("interesting")
            .and_then(|a| a.as_noul())
            .map(|(p, _)| p >= 0.5);
        let risk = response
            .answer("risk")
            .and_then(|a| a.as_score())
            .map(|(s, _)| (s / 4.0).clamp(0.0, 1.0))
            .unwrap_or(0.3);
        Self {
            accessible,
            auth_required,
            interesting,
            risk,
            confidence: response.min_confidence(),
            provider: response.provider.clone(),
        }
    }
}

pub fn heuristic_endpoint(url: &str, method: &str) -> EndpointTriage {
    let lower = url.to_ascii_lowercase();
    let role = if lower.contains("graphql") || lower.contains("/gql") {
        "graphql"
    } else if lower.contains("ws") || lower.contains("socket") {
        "websocket"
    } else if lower.contains("admin") || lower.contains("internal") || lower.contains("debug") {
        "admin"
    } else if lower.contains("auth")
        || lower.contains("login")
        || lower.contains("oauth")
        || lower.contains("token")
    {
        "auth"
    } else if lower.contains("upload") || lower.contains("import") {
        "upload"
    } else if lower.contains("/api")
        || lower.contains("/v1")
        || lower.contains("/v2")
        || lower.contains("/rest")
    {
        "api"
    } else if lower.ends_with(".js")
        || lower.ends_with(".css")
        || lower.ends_with(".json")
        || lower.ends_with(".map")
    {
        "static"
    } else if lower.contains("swagger") || lower.contains("openapi") || lower.contains("docs") {
        "docs"
    } else if lower.contains("config") || lower.contains("manifest") || lower.contains(".env") {
        "config"
    } else {
        "page"
    };
    let sensitivity = match role {
        "admin" => 0.9,
        "auth" => 0.75,
        "upload" => 0.7,
        "graphql" => 0.65,
        "api" => 0.55,
        "config" => 0.6,
        "websocket" => 0.6,
        "docs" => 0.35,
        "static" => 0.15,
        _ => 0.3,
    };
    let priority = match role {
        "admin" => 0.95,
        "auth" => 0.8,
        "upload" => 0.75,
        "graphql" => 0.7,
        "api" => 0.65,
        "config" => 0.6,
        "websocket" => 0.65,
        "docs" => 0.4,
        "static" => 0.1,
        _ => 0.35,
    };
    EndpointTriage {
        role: role.into(),
        sensitivity,
        auth_required: Some(matches!(role, "admin" | "auth" | "upload")),
        verify_safe: Some(matches!(
            method.to_ascii_uppercase().as_str(),
            "GET" | "HEAD" | "OPTIONS"
        )),
        priority,
        confidence: 0.35,
        provider: "heuristic".into(),
    }
}

pub fn heuristic_secret(severity: &str, entropy: f32) -> SecretTriage {
    let risk = match severity {
        "critical" => 0.95,
        "high" => 0.8,
        "medium" => 0.55,
        "low" => 0.3,
        _ => 0.2,
    };
    SecretTriage {
        is_real: Some(entropy >= 3.0),
        secret_class: "generic".into(),
        risk,
        should_validate: Some(false),
        confidence: 0.35,
        provider: "heuristic".into(),
    }
}

pub fn heuristic_asset(kind: &str) -> AssetTriage {
    AssetTriage {
        role: kind.into(),
        worth_deep_parse: Some(matches!(
            kind,
            "javascript" | "source_map" | "json" | "graphql"
        )),
        contains_auth_logic: None,
        contains_graphql: None,
        confidence: 0.35,
        provider: "heuristic".into(),
    }
}
