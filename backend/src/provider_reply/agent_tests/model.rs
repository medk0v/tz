use super::super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scenario {
    pub name: String,
    pub language: String,
    pub channel_id: Option<Uuid>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub contact: Option<ScenarioContact>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub knowledge_articles: Vec<ScenarioArticle>,
    #[serde(default)]
    pub history: Vec<History>,
    #[serde(default)]
    pub operator_present: bool,
    #[serde(default)]
    pub closed: bool,
    pub timer_seconds: Option<i64>,
    pub steps: Vec<Step>,
    #[serde(default)]
    pub fixtures: Vec<Fixture>,
    #[serde(default)]
    pub live_allowlist: Vec<ToolCall>,
    #[serde(default)]
    pub source_articles: Vec<Uuid>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioContact {
    /// Artificial card values; the runner never looks up or creates a contact.
    pub contact_id: Uuid,
    pub display_name: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ScenarioArticle {
    pub article_id: Uuid,
    pub title: String,
    pub body: String,
    #[serde(default = "default_article_version")]
    pub version: i64,
}

fn default_article_version() -> i64 {
    1
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct History {
    pub author: String,
    pub text: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Step {
    #[serde(default)]
    pub message: String,
    #[serde(default)]
    pub advance_seconds: i64,
    pub operator_present: Option<bool>,
    pub closed: Option<bool>,
    #[serde(default)]
    pub expected_meaning: String,
    #[serde(default)]
    pub required_facts: Vec<String>,
    #[serde(default)]
    pub forbidden_facts: Vec<String>,
    #[serde(default)]
    pub required_actions: Vec<ToolCall>,
    #[serde(default)]
    pub forbidden_actions: Vec<ToolCall>,
    pub expected_timer_seconds: Option<i64>,
    pub expected_timer_active: Option<bool>,
    pub expected_closed: Option<bool>,
    pub expected_reply: Option<bool>,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCall {
    pub tool: String,
    pub parameters: Value,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Fixture {
    pub call: ToolCall,
    /// Exact tool stdout. Integration stdout uses the production envelope.
    pub response: String,
    #[serde(default = "default_uses")]
    pub uses: usize,
}
fn default_uses() -> usize {
    1
}

impl Scenario {
    /// Scenario-level permission; the runner also enforces the agent's permissions.
    pub(super) fn allows_live_call(&self, call: &ToolCall) -> bool {
        if self.live_allowlist.is_empty() {
            call.tool == "browser" || (call.tool == "http" && call.parameters["method"] == "GET")
        } else {
            self.live_allowlist.contains(call)
        }
    }

    pub fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            !self.name.trim().is_empty() && self.name.len() <= 300,
            "scenario name is required (maximum 300 bytes)"
        );
        anyhow::ensure!(
            ReplyLanguage::parse(&self.language).is_some(),
            "invalid scenario language"
        );
        anyhow::ensure!(
            self.contact
                .as_ref()
                .and_then(|contact| contact.display_name.as_ref())
                .is_none_or(|name| name.len() <= 1000),
            "contact display name exceeds 1000 bytes"
        );
        anyhow::ensure!(
            (1..=20).contains(&self.steps.len())
                && self.history.len() <= 100
                && self.knowledge_articles.len() <= 20
                && self.fixtures.len() <= 100
                && self.live_allowlist.len() <= 32,
            "scenario limits exceeded"
        );
        let mut article_ids = HashSet::new();
        for article in &self.knowledge_articles {
            anyhow::ensure!(
                article_ids.insert(article.article_id),
                "scenario knowledge article IDs must be unique"
            );
            anyhow::ensure!(
                !article.title.trim().is_empty() && article.title.chars().count() <= 300,
                "scenario knowledge title is required (maximum 300 characters)"
            );
            anyhow::ensure!(
                !article.body.trim().is_empty() && article.body.chars().count() <= 200_000,
                "scenario knowledge body is required (maximum 200000 characters)"
            );
            anyhow::ensure!(
                article.version > 0,
                "scenario knowledge version must be positive"
            );
        }
        anyhow::ensure!(
            self.history.iter().all(
                |h| ["contact", "ai", "operator"].contains(&h.author.as_str())
                    && h.text.len() <= 20_000
            ),
            "invalid history"
        );
        if let Some(seconds) = self.timer_seconds {
            ScheduledReminder {
                delay_seconds: seconds,
            }
            .validate()?;
        }
        for step in &self.steps {
            anyhow::ensure!(
                (0..=86_400).contains(&step.advance_seconds) && step.message.len() <= 20_000,
                "invalid step duration or message"
            );
            anyhow::ensure!(
                step.required_facts
                    .iter()
                    .chain(&step.forbidden_facts)
                    .all(|f| !f.trim().is_empty()),
                "fact conditions must not be empty"
            );
            for call in step.required_actions.iter().chain(&step.forbidden_actions) {
                call.validate()?;
            }
        }
        for fixture in &self.fixtures {
            fixture.call.validate()?;
            anyhow::ensure!(
                (1..=32).contains(&fixture.uses) && fixture.response.len() <= 128 * 1024,
                "invalid fixture size or uses"
            );
            if fixture.call.tool == "integration" {
                let response: Value = serde_json::from_str(&fixture.response)?;
                anyhow::ensure!(
                    response == json!({"ok":false,"error":"integration_unavailable"})
                        || (response["ok"] == true
                            && response["body"].is_string()
                            && response["status_code"]
                                .as_u64()
                                .is_some_and(|n| (100..=599).contains(&n))
                            && ["application/json", "text/plain"]
                                .contains(&response["content_type"].as_str().unwrap_or_default())),
                    "invalid integration fixture envelope"
                );
            }
        }
        for call in &self.live_allowlist {
            call.validate()?;
            anyhow::ensure!(
                ["integration", "http", "browser"].contains(&call.tool.as_str()),
                "live requests must use http, browser or integration"
            );
        }
        anyhow::ensure!(
            serde_json::to_vec(self)?.len() <= 512 * 1024,
            "scenario exceeds 512 KiB"
        );
        Ok(())
    }
}
impl ToolCall {
    fn validate(&self) -> Result<()> {
        anyhow::ensure!(
            [
                "integration",
                "http",
                "browser",
                "shell",
                "notify_operator",
                "resolve",
                "schedule_reminder",
                "read_article",
                "timer_cancelled",
                "timer_fired"
            ]
            .contains(&self.tool.as_str()),
            "unknown test tool"
        );
        anyhow::ensure!(
            self.parameters.is_object(),
            "tool parameters must be an object"
        );
        Ok(())
    }
}

#[derive(Default, Serialize)]
pub struct TestResult {
    pub steps: Vec<Value>,
    pub trace: Vec<Value>,
    pub failures: Vec<String>,
    pub execution_errors: Vec<String>,
    pub semantic_review: Vec<Value>,
    pub review_history: Vec<Value>,
}

impl TestResult {
    pub fn status(&self) -> &'static str {
        if !self.execution_errors.is_empty() {
            "execution_error"
        } else if !self.failures.is_empty()
            || self.semantic_review.iter().any(|v| v["verdict"] == "fail")
        {
            "behavior_error"
        } else if self.semantic_review.is_empty()
            || self.semantic_review.iter().any(|v| v["verdict"] != "pass")
        {
            "manual_review"
        } else {
            "passed"
        }
    }
}

fn action_matches(expected: &ToolCall, actual: &ToolCall) -> bool {
    // Existing scenarios constrain whether a handoff happens, without prescribing
    // the model's wording for the newly supported operator summary.
    expected == actual
        || (expected.tool == "notify_operator"
            && actual.tool == "notify_operator"
            && expected.parameters == json!({}))
}

pub fn evaluate_step(
    step: &Step,
    reply: &str,
    actions: &[ToolCall],
    timer: Option<(i64, i64)>,
    now: i64,
    closed: bool,
) -> Vec<String> {
    let mut failures = Vec::new();
    for fact in &step.required_facts {
        if !reply.contains(fact) {
            failures.push(format!("required literal fact absent: {fact}"));
        }
    }
    for fact in &step.forbidden_facts {
        if reply.contains(fact) {
            failures.push(format!("forbidden literal fact present: {fact}"));
        }
    }
    for action in &step.required_actions {
        if !actions.iter().any(|actual| action_matches(action, actual)) {
            failures.push(format!("required action absent: {}", action.tool));
        }
    }
    for action in &step.forbidden_actions {
        if actions.iter().any(|actual| action_matches(action, actual)) {
            failures.push(format!("forbidden action performed: {}", action.tool));
        }
    }
    if step
        .expected_timer_active
        .is_some_and(|active| active != timer.is_some())
    {
        failures.push("unexpected timer state".into());
    }
    if step
        .expected_timer_seconds
        .is_some_and(|seconds| timer.map(|(due, _)| due - now) != Some(seconds))
    {
        failures.push("unexpected timer remaining seconds".into());
    }
    if step
        .expected_closed
        .is_some_and(|expected| expected != closed)
    {
        failures.push("unexpected conversation state".into());
    }
    if step
        .expected_reply
        .is_some_and(|expected| expected == reply.is_empty())
    {
        failures.push("unexpected reply presence".into());
    }
    failures
}

/// Trace data is untrusted. Drop secret/reasoning fields, including inside JSON strings.
pub fn redact(value: Value, secrets: &[String]) -> Value {
    match value {
        Value::Object(values) => Value::Object(
            values
                .into_iter()
                .map(|(key, value)| {
                    let normalized = key.to_lowercase().replace(['-', '_'], "");
                    let sensitive = [
                        "secret",
                        "token",
                        "password",
                        "authorization",
                        "apikey",
                        "cookie",
                        "reasoning",
                        "thought",
                        "analysis",
                        "chainofthought",
                        "encrypted",
                        "nonce",
                    ]
                    .iter()
                    .any(|word| normalized.contains(word))
                        && ![
                            "maxoutputtokens",
                            "maxtokens",
                            "maxcompletiontokens",
                            "inputtokens",
                            "outputtokens",
                            "totaltokens",
                        ]
                        .contains(&normalized.as_str());
                    (
                        key,
                        if sensitive {
                            json!("[REDACTED]")
                        } else {
                            redact(value, secrets)
                        },
                    )
                })
                .collect(),
        ),
        Value::Array(values) => {
            Value::Array(values.into_iter().map(|v| redact(v, secrets)).collect())
        }
        Value::String(mut text) => {
            if let Ok(parsed) = serde_json::from_str::<Value>(&text)
                && (parsed.is_object() || parsed.is_array())
            {
                return json!(redact(parsed, secrets).to_string());
            }
            for secret in secrets.iter().filter(|s| !s.is_empty()) {
                text = text.replace(secret, "[REDACTED]");
            }
            // Never retain model reasoning blocks in plain text responses either.
            for tag in ["think", "analysis", "reasoning"] {
                while let Some(start) = text.to_ascii_lowercase().find(&format!("<{tag}>")) {
                    let end = text[start..]
                        .to_ascii_lowercase()
                        .find(&format!("</{tag}>"));
                    let end = end.map_or(text.len(), |end| start + end + tag.len() + 3);
                    text.replace_range(start..end, "[REDACTED]");
                }
            }
            json!(text)
        }
        other => other,
    }
}
