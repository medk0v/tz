//! Current source text editing from a completed run, without replacing unrelated settings.
use super::*;
use sqlx::{Postgres, Transaction};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Source {
    Instructions,
    ToolInstructions,
    Article(Uuid),
}

impl Source {
    pub(super) fn parse(key: &str) -> Result<Self, AppError> {
        match key {
            "instructions" => Ok(Self::Instructions),
            "tool_instructions" => Ok(Self::ToolInstructions),
            _ => key
                .strip_prefix("article:")
                .and_then(|id| Uuid::parse_str(id).ok())
                .map(Self::Article)
                .ok_or_else(|| AppError::BadRequest("unsupported instruction source".into())),
        }
    }

    pub(super) fn available_in(self, run: &Value) -> bool {
        match self {
            Self::Instructions => run["snapshot"]["profile"]["instructions"].is_string(),
            Self::ToolInstructions => run["snapshot"]["profile"]["tool_instructions"].is_string(),
            Self::Article(id) => {
                if run["snapshot"]["scenario"]["knowledge_articles"]
                    .as_array()
                    .is_some_and(|articles| {
                        articles
                            .iter()
                            .any(|article| article["article_id"] == id.to_string())
                    })
                {
                    return false;
                }
                let Some(article) = run["snapshot"]["knowledge"]
                    .as_array()
                    .and_then(|articles| {
                        articles
                            .iter()
                            .find(|article| article["article_id"] == id.to_string())
                    })
                else {
                    return false;
                };
                run["result"]["trace"].as_array().is_some_and(|trace| {
                    trace.iter().any(|entry| {
                        entry["call"]["tool"] == "read_article"
                            && entry["call"]["parameters"]["article_id"] == id.to_string()
                            && entry["call"]["parameters"]["version"] == article["version"]
                            && entry["response"]["article_id"] == article["article_id"]
                            && entry["response"]["version"] == article["version"]
                            && entry["response"]["content"]
                                .as_str()
                                .is_some_and(|text| !text.trim().is_empty())
                    })
                })
            }
        }
    }

    fn key(self) -> String {
        match self {
            Self::Instructions => "instructions".into(),
            Self::ToolInstructions => "tool_instructions".into(),
            Self::Article(id) => format!("article:{id}"),
        }
    }

    pub(super) fn validate_length(self, text: &str) -> Result<(), AppError> {
        let max = if matches!(self, Self::Article(_)) {
            200_000
        } else {
            50_000
        };
        if text.chars().count() > max {
            return Err(AppError::BadRequest(format!(
                "instruction source must not exceed {max} characters"
            )));
        }
        Ok(())
    }

    fn normalize_text(self, text: String) -> Result<String, AppError> {
        let text = text.trim();
        self.validate_length(text)?;
        Ok(text.to_owned())
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct UpdateSource {
    text: String,
    expected_revision: String,
}

#[derive(Serialize)]
pub(super) struct SourceText {
    text: String,
    revision: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    status: Option<String>,
}

impl SourceText {
    fn instructions(text: String) -> Self {
        Self {
            revision: format!("{:x}", Sha256::digest(text.as_bytes())),
            text,
            title: None,
            status: None,
        }
    }
}

async fn lock_run(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    run_id: Uuid,
) -> Result<Value, AppError> {
    ai_settings::lock_profile_for_management_scope(tx, actor, project, profile).await?;
    ai_settings::require_project_wide_for_autonomous_profile(tx, actor, project, profile).await?;
    let run = sqlx::query_scalar::<_, Value>(
        "SELECT jsonb_build_object('snapshot',snapshot,'result',result,'status',status) FROM ai_test_runs WHERE tenant_id=$1 AND project_id=$2 AND profile_id=$3 AND id=$4 AND COALESCE(snapshot->>'knowledge_project_id',project_id::text)=$5 FOR SHARE",
    )
    .bind(actor.tenant_id).bind(project).bind(profile).bind(run_id).bind(ai_settings::require_management(actor)?.to_string())
    .fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)?;
    if run["status"] == "running" {
        return Err(AppError::Conflict(
            "test is still running; wait for it to finish".into(),
        ));
    }
    Ok(run)
}

async fn article_base(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    article_id: Uuid,
) -> Result<Uuid, AppError> {
    actor.require("knowledge:manage")?;
    sqlx::query_scalar::<_, Uuid>(
        r#"SELECT a.knowledge_base_id FROM knowledge_articles a
           JOIN knowledge_bases b ON b.tenant_id=a.tenant_id AND b.project_id=a.project_id AND b.id=a.knowledge_base_id
           JOIN ai_profile_knowledge_bases x ON x.tenant_id=b.tenant_id AND x.knowledge_base_id=b.id
           WHERE a.tenant_id=$1 AND a.project_id=$2 AND x.ai_profile_id=$3 AND a.id=$4 AND b.status='active'"#,
    )
    .bind(actor.tenant_id).bind(project).bind(profile).bind(article_id)
    .fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)
}

async fn lock_current(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    source: Source,
) -> Result<SourceText, AppError> {
    if let Source::Article(article_id) = source {
        let base = article_base(tx, actor, project, profile, article_id).await?;
        crate::knowledge::lock_knowledge_base_for_task_scope(tx, actor, project, base).await?;
        let (text, version, title, status): (String, i64, String, String) = sqlx::query_as(
            r#"SELECT a.body,a.version,a.title,a.status FROM knowledge_articles a
               JOIN knowledge_bases b ON b.tenant_id=a.tenant_id AND b.project_id=a.project_id AND b.id=a.knowledge_base_id
               WHERE a.tenant_id=$1 AND a.project_id=$2 AND a.knowledge_base_id=$3 AND a.id=$4 AND b.status='active' FOR UPDATE OF a"#,
        )
        .bind(actor.tenant_id).bind(project).bind(base).bind(article_id)
        .fetch_optional(&mut **tx).await?.ok_or(AppError::NotFound)?;
        Ok(SourceText {
            text,
            revision: version.to_string(),
            title: Some(title),
            status: Some(status),
        })
    } else {
        let text = sqlx::query_scalar::<_, String>(
            "SELECT CASE WHEN $4 THEN instructions ELSE tool_instructions END FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
        )
        .bind(actor.tenant_id).bind(project).bind(profile).bind(source == Source::Instructions)
        .fetch_one(&mut **tx).await?;
        Ok(SourceText::instructions(text))
    }
}

async fn lock_source(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    run_id: Uuid,
    source: Source,
) -> Result<SourceText, AppError> {
    let run = lock_run(tx, actor, project, profile, run_id).await?;
    if !source.available_in(&run) {
        return Err(AppError::NotFound);
    }
    lock_current(tx, actor, project, profile, source).await
}

pub(super) async fn get(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, run, key)): RoutePath<(Uuid, Uuid, String)>,
) -> Result<Json<SourceText>, AppError> {
    ai_settings::require_management(&actor)?;
    let project = crate::resource_visibility::owner_project(
        &state.db,
        &actor,
        crate::resource_visibility::Resource::Profile,
        profile,
    )
    .await?;
    let source = Source::parse(&key)?;
    let mut tx = state.db.begin().await?;
    let current = lock_source(&mut tx, &actor, project, profile, run, source).await?;
    tx.commit().await?;
    Ok(Json(current))
}

pub(super) async fn update(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, run, key)): RoutePath<(Uuid, Uuid, String)>,
    Json(input): Json<UpdateSource>,
) -> Result<Json<SourceText>, AppError> {
    ai_settings::require_management(&actor)?;
    let project = crate::resource_visibility::owner_project(
        &state.db,
        &actor,
        crate::resource_visibility::Resource::Profile,
        profile,
    )
    .await?;
    let source = Source::parse(&key)?;
    let text = source.normalize_text(input.text)?;
    let mut tx = state.db.begin().await?;
    let current = lock_source(&mut tx, &actor, project, profile, run, source).await?;
    if current.revision != input.expected_revision {
        return Err(AppError::Conflict(
            "instruction source was changed; reload it before saving".into(),
        ));
    }
    if current.text == text {
        tx.commit().await?;
        return Ok(Json(current));
    }
    let (updated, action, resource_kind, resource_id) =
        save_source(&mut tx, &actor, project, profile, source, text, current).await?;
    sqlx::query(
        "INSERT INTO audit_log(id,tenant_id,project_id,actor_id,action,resource_kind,resource_id,metadata) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project).bind(actor.actor_id)
    .bind(action).bind(resource_kind).bind(resource_id)
    .bind(json!({"test_run_id":run,"source_key":key}))
    .execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(Json(updated))
}

async fn save_source(
    tx: &mut Transaction<'_, Postgres>,
    actor: &ActorContext,
    project: Uuid,
    profile: Uuid,
    source: Source,
    text: String,
    current: SourceText,
) -> Result<(SourceText, &'static str, &'static str, Uuid), AppError> {
    let result = if let Source::Article(id) = source {
        let version: i64 = sqlx::query_scalar(
            "UPDATE knowledge_articles SET body=$4,updated_by=$5,updated_at=now(),version=version+1 WHERE tenant_id=$1 AND project_id=$2 AND id=$3 RETURNING version",
        )
        .bind(actor.tenant_id).bind(project).bind(id).bind(&text).bind(actor.actor_id)
        .fetch_one(&mut **tx).await?;
        (
            SourceText {
                text,
                revision: version.to_string(),
                ..current
            },
            "knowledge_article.updated",
            "knowledge_article",
            id,
        )
    } else {
        let active: bool = sqlx::query_scalar(
            "SELECT status='active' FROM ai_profiles WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
        )
        .bind(actor.tenant_id).bind(project).bind(profile).fetch_one(&mut **tx).await?;
        if source == Source::Instructions && active && text.is_empty() {
            return Err(AppError::BadRequest(
                "an active profile requires instructions".into(),
            ));
        }
        sqlx::query(
            "UPDATE ai_profiles SET instructions=CASE WHEN $4 THEN $5 ELSE instructions END,tool_instructions=CASE WHEN $4 THEN tool_instructions ELSE $5 END,updated_at=now() WHERE tenant_id=$1 AND project_id=$2 AND id=$3",
        )
        .bind(actor.tenant_id).bind(project).bind(profile).bind(source == Source::Instructions).bind(&text)
        .execute(&mut **tx).await?;
        (
            SourceText::instructions(text),
            "ai_profile.updated",
            "ai_profile",
            profile,
        )
    };
    Ok(result)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ApplyRecommendations {
    changes: Vec<RecommendedChange>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RecommendedChange {
    source_key: String,
    expected_revision: String,
    original_text: String,
    suggested_text: String,
}

#[derive(Serialize)]
pub(super) struct AppliedRecommendations {
    sources: Vec<AppliedSource>,
}

#[derive(Serialize)]
struct AppliedSource {
    source_key: String,
    #[serde(flatten)]
    current: SourceText,
}

/// Apply simultaneous replacements against original offsets, preserving every unrelated byte.
pub(super) fn replace_fragments(text: &str, changes: &[(&str, &str)]) -> Result<String, AppError> {
    if changes.is_empty() || changes.len() > 5 {
        return Err(AppError::BadRequest(
            "submit 1 to 5 recommendation changes".into(),
        ));
    }
    let mut replacements = Vec::with_capacity(changes.len());
    for &(original, suggested) in changes {
        if original.trim().is_empty()
            || suggested.trim().is_empty()
            || original == suggested
            || original.chars().count() > 12_000
            || suggested.chars().count() > 12_000
        {
            return Err(AppError::BadRequest(
                "recommendation fragments must be distinct nonempty text of at most 12000 characters".into(),
            ));
        }
        let start = text.find(original).ok_or_else(|| {
            AppError::Conflict(
                "recommended original text has changed; generate recommendations again".into(),
            )
        })?;
        // Advance by one character, not the match length: overlapping matches are ambiguous too.
        let next = start + original.chars().next().map_or(0, char::len_utf8);
        if text[next..].contains(original) {
            return Err(AppError::Conflict(
                "recommended original text appears more than once; generate recommendations again"
                    .into(),
            ));
        }
        replacements.push((start, start + original.len(), suggested));
    }
    replacements.sort_unstable_by_key(|&(start, _, _)| start);
    let mut replaced = String::with_capacity(text.len());
    let mut end = 0;
    for (start, next_end, suggested) in replacements {
        if start < end {
            return Err(AppError::Conflict(
                "recommendation fragments overlap; generate recommendations again".into(),
            ));
        }
        replaced.push_str(&text[end..start]);
        replaced.push_str(suggested);
        end = next_end;
    }
    replaced.push_str(&text[end..]);
    Ok(replaced)
}

pub(super) async fn apply_recommendations(
    State(state): State<AppState>,
    actor: ActorContext,
    RoutePath((profile, run_id)): RoutePath<(Uuid, Uuid)>,
    Json(input): Json<ApplyRecommendations>,
) -> Result<Json<AppliedRecommendations>, AppError> {
    ai_settings::require_management(&actor)?;
    let project = crate::resource_visibility::owner_project(
        &state.db,
        &actor,
        crate::resource_visibility::Resource::Profile,
        profile,
    )
    .await?;
    if input.changes.is_empty() || input.changes.len() > 5 {
        return Err(AppError::BadRequest(
            "submit 1 to 5 recommendation changes".into(),
        ));
    }
    let mut grouped: BTreeMap<Source, Vec<RecommendedChange>> = BTreeMap::new();
    for change in input.changes {
        grouped
            .entry(Source::parse(&change.source_key)?)
            .or_default()
            .push(change);
    }
    let mut tx = state.db.begin().await?;
    let run = lock_run(&mut tx, &actor, project, profile, run_id).await?;
    // Base IDs and then article IDs must be locked in a shared order, even across profiles.
    let mut bases = BTreeSet::new();
    for &source in grouped.keys() {
        if !source.available_in(&run) {
            return Err(AppError::NotFound);
        }
        if let Source::Article(id) = source {
            bases.insert(article_base(&mut tx, &actor, project, profile, id).await?);
        }
    }
    for base in bases {
        crate::knowledge::lock_knowledge_base_for_task_scope(&mut tx, &actor, project, base)
            .await?;
    }
    let mut prepared = Vec::with_capacity(grouped.len());
    for (source, changes) in grouped {
        let current = lock_current(&mut tx, &actor, project, profile, source).await?;
        if changes
            .iter()
            .any(|change| change.expected_revision != current.revision)
        {
            return Err(AppError::Conflict(
                "instruction source was changed; generate recommendations again".into(),
            ));
        }
        let fragments = changes
            .iter()
            .map(|change| {
                (
                    change.original_text.as_str(),
                    change.suggested_text.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let text = replace_fragments(&current.text, &fragments)?;
        source.validate_length(&text)?;
        prepared.push((source, text, current, changes.len()));
    }
    let mut sources = Vec::with_capacity(prepared.len());
    for (source, text, current, change_count) in prepared {
        let (current, action, resource_kind, resource_id) =
            save_source(&mut tx, &actor, project, profile, source, text, current).await?;
        let source_key = source.key();
        sqlx::query(
            "INSERT INTO audit_log(id,tenant_id,project_id,actor_id,action,resource_kind,resource_id,metadata) VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(Uuid::now_v7()).bind(actor.tenant_id).bind(project).bind(actor.actor_id)
        .bind(action).bind(resource_kind).bind(resource_id)
        .bind(json!({"test_run_id":run_id,"source_key":source_key,"recommendations_applied":change_count}))
        .execute(&mut *tx).await?;
        sources.push(AppliedSource {
            source_key,
            current,
        });
    }
    tx.commit().await?;
    Ok(Json(AppliedRecommendations { sources }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommendation_replacements_preserve_surrounding_text_and_original_offsets() {
        let text = "  Правило A: первый.\nПравило B: второй.\n  ";
        let updated =
            replace_fragments(text, &[("второй", "готово"), ("первый", "второй и третий")])
                .unwrap();
        assert_eq!(
            updated,
            "  Правило A: второй и третий.\nПравило B: готово.\n  "
        );
        assert_eq!(
            replace_fragments("ab", &[("a", "b"), ("b", "a")]).unwrap(),
            "ba"
        );
    }

    #[test]
    fn recommendation_replacements_reject_missing_duplicate_and_overlapping_matches() {
        for (text, changes) in [
            ("abc", vec![("missing", "replacement")]),
            ("abc abc", vec![("abc", "replacement")]),
            ("aaa", vec![("aa", "replacement")]),
            ("яяя", vec![("яя", "replacement")]),
            ("abcd", vec![("abc", "replacement"), ("bcd", "other")]),
            ("abc", vec![("abc", "replacement"), ("abc", "other")]),
        ] {
            assert!(matches!(
                replace_fragments(text, &changes),
                Err(AppError::Conflict(_))
            ));
        }
    }

    #[test]
    fn recommendation_replacements_require_bounded_distinct_nonempty_fragments() {
        for changes in [
            vec![],
            vec![("", "x")],
            vec![("a", " ")],
            vec![("a", "a")],
            vec![("a", "b"); 6],
        ] {
            assert!(matches!(
                replace_fragments("a", &changes),
                Err(AppError::BadRequest(_))
            ));
        }
        assert!(replace_fragments("a", &[("a", &"x".repeat(12_001))]).is_err());
    }

    #[test]
    fn only_successfully_read_snapshot_article_versions_are_editable() {
        let id = Uuid::now_v7();
        let source = Source::Article(id);
        let mut run = json!({
            "snapshot":{"knowledge":[{"article_id":id,"version":3}]},
            "result":{"trace":[{"call":{"tool":"read_article","parameters":{"article_id":id,"version":3}},"response":{"article_id":id,"version":3,"content":"Read text"}}]}
        });
        assert!(source.available_in(&run));
        run["result"]["trace"][0]["response"] = Value::Null;
        assert!(!source.available_in(&run));
        run["result"]["trace"][0]["response"] = json!({"article_id":id,"version":3,"content":"  "});
        assert!(!source.available_in(&run));
        run["result"]["trace"][0]["response"]["content"] = json!("Read text");
        run["result"]["trace"][0]["response"]["article_id"] = json!(Uuid::now_v7());
        assert!(!source.available_in(&run));
        run["result"]["trace"][0]["response"]["article_id"] = json!(id);
        run["result"]["trace"][0]["response"]["version"] = json!(4);
        assert!(!source.available_in(&run));
        run["result"]["trace"][0]["response"]["version"] = json!(3);
        run["result"]["trace"][0]["call"]["parameters"]["version"] = json!(4);
        assert!(!source.available_in(&run));
        run["result"]["trace"] = json!([]);
        assert!(!source.available_in(&run));
    }

    #[test]
    fn source_keys_and_limits_do_not_allow_other_profile_fields() {
        assert!(Source::parse("model").is_err());
        assert!(Source::parse("article:invalid").is_err());
        assert!(
            Source::Instructions
                .normalize_text("x".repeat(50_001))
                .is_err()
        );
        assert!(
            Source::ToolInstructions
                .normalize_text("x".repeat(50_001))
                .is_err()
        );
        assert!(
            Source::Article(Uuid::now_v7())
                .normalize_text("x".repeat(200_001))
                .is_err()
        );
        assert_eq!(
            Source::Instructions
                .normalize_text("  Text  ".into())
                .unwrap(),
            "Text"
        );
    }

    #[test]
    fn scenario_articles_never_authorize_published_source_editing() {
        let id = Uuid::from_u128(42);
        let mut run = json!({
            "snapshot":{"scenario":{"knowledge_articles":[{"article_id":id,"version":1,"title":"Test article","body":"Test response"}]},"knowledge":[]},
            "result":{"trace":[{"call":{"tool":"read_article","parameters":{"article_id":id,"version":1}},"response":{"article_id":id,"version":1,"content":"Test response"}}]}
        });
        assert!(!Source::Article(id).available_in(&run));
        run["snapshot"]["knowledge"] = json!([{"article_id":id,"version":1}]);
        assert!(!Source::Article(id).available_in(&run));
    }
}
