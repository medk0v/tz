use super::*;

#[sqlx::test]
#[ignore = "requires a PostgreSQL role that can create disposable test databases"]
async fn shared_agent_snapshot_uses_selected_project_skills(db: sqlx::PgPool) {
    let tenant = Uuid::from_u128(1);
    let owner = Uuid::from_u128(2);
    let selected = Uuid::from_u128(3);
    let user = Uuid::from_u128(4);
    let profile = Uuid::from_u128(5);
    let base = Uuid::from_u128(6);
    let article = Uuid::from_u128(7);
    sqlx::raw_sql(&format!(
        r#"
        INSERT INTO tenants (id,name) VALUES ('{tenant}','Test skills');
        INSERT INTO users (id,email,display_name) VALUES ('{user}','snapshot@skills.test','Admin');
        INSERT INTO projects (id,tenant_id,name,slug) VALUES
          ('{owner}','{tenant}','Owner','owner'),
          ('{selected}','{tenant}','Selected','selected');
        INSERT INTO ai_profiles (id,tenant_id,project_id,name,instructions,status,created_by)
          VALUES ('{profile}','{tenant}','{owner}','Shared','Main instructions','active','{user}');
        INSERT INTO knowledge_bases (id,tenant_id,project_id,name,created_by)
          VALUES ('{base}','{tenant}','{owner}','Owner knowledge','{user}');
        INSERT INTO knowledge_articles (id,tenant_id,project_id,knowledge_base_id,title,body,status,published_at,created_by,updated_by)
          VALUES ('{article}','{tenant}','{owner}','{base}','Owner article','Private owner policy','published',now(),'{user}','{user}');
        INSERT INTO ai_profile_knowledge_bases (tenant_id,ai_profile_id,knowledge_base_id)
          VALUES ('{tenant}','{profile}','{base}');
        "#
    ))
    .execute(&db)
    .await
    .unwrap();
    for (project, name) in [(owner, "Owner policy"), (selected, "Selected policy")] {
        let skill = Uuid::now_v7();
        sqlx::query("INSERT INTO ai_skills (id,tenant_id,project_id,name,instructions) VALUES ($1,$2,$3,$4,$4)")
            .bind(skill).bind(tenant).bind(project).bind(name).execute(&db).await.unwrap();
        sqlx::query("INSERT INTO ai_profile_skills (tenant_id,project_id,skill_id,ai_profile_id) VALUES ($1,$2,$3,$4)")
            .bind(tenant).bind(project).bind(skill).bind(profile).execute(&db).await.unwrap();
    }
    let mut config: crate::Config =
        toml::from_str(include_str!("../../../Config.example.toml")).unwrap();
    config.pg.migrate_on_start = false;
    config.redis.url = None;
    config.attachments.enabled = false;
    let mut state = AppState::build(config).await.unwrap();
    state.db = db.clone();

    let saved = snapshot(&state, tenant, owner, profile, selected)
        .await
        .unwrap();
    assert_eq!(saved["profile"]["project_id"], json!(owner));
    assert_eq!(saved["skill_project_id"], json!(selected));
    assert_eq!(saved["skills"].as_array().unwrap().len(), 1);
    assert_eq!(saved["skills"][0]["instructions"], "Selected policy");
    assert_eq!(saved["knowledge"], json!([]));
    let owner_snapshot = snapshot(&state, tenant, owner, profile, owner)
        .await
        .unwrap();
    assert_eq!(owner_snapshot["knowledge"][0]["article_id"], json!(article));
    let owner_catalog = load_knowledge_catalog(&state, tenant, owner, profile)
        .await
        .unwrap();
    assert_eq!(owner_catalog.len(), 1);
    assert_eq!(owner_catalog[0].id, article);
    assert!(
        load_knowledge_catalog(&state, tenant, selected, profile)
            .await
            .unwrap()
            .is_empty()
    );
    assert!(
        load_knowledge_article(&state, tenant, selected, profile, article, None)
            .await
            .unwrap()
            .is_none()
    );
    assert_eq!(
        load_knowledge_article(&state, tenant, owner, profile, article, Some(1))
            .await
            .unwrap()
            .unwrap()
            .content,
        "Private owner policy"
    );
    let mut messages = build_chat_messages("Main instructions", None, None, &[], &[]);
    append_profile_skills(
        &mut messages,
        &serde_json::from_value::<Vec<ai_skills::RuntimeSkill>>(saved["skills"].clone()).unwrap(),
    );
    let prompt = messages[0].content.as_str().unwrap();
    assert!(prompt.contains("Selected policy"));
    assert!(!prompt.contains("Owner policy"));

    sqlx::query("UPDATE ai_skills SET instructions='Changed owner policy' WHERE project_id=$1")
        .bind(owner)
        .execute(&db)
        .await
        .unwrap();
    let current = snapshot(&state, tenant, owner, profile, selected)
        .await
        .unwrap();
    assert_eq!(fingerprint(&saved), fingerprint(&current));
    sqlx::query("UPDATE ai_skills SET instructions='Changed selected policy' WHERE project_id=$1")
        .bind(selected)
        .execute(&db)
        .await
        .unwrap();
    let changed = snapshot(&state, tenant, owner, profile, selected)
        .await
        .unwrap();
    assert_ne!(fingerprint(&saved), fingerprint(&changed));

    sqlx::query("DELETE FROM ai_skills WHERE tenant_id=$1")
        .bind(tenant)
        .execute(&db)
        .await
        .unwrap();
    let empty_selected = snapshot(&state, tenant, owner, profile, selected)
        .await
        .unwrap();
    let empty_owner = snapshot(&state, tenant, owner, profile, owner)
        .await
        .unwrap();
    assert_eq!(empty_selected["skills"], json!([]));
    assert_eq!(empty_owner["skills"], json!([]));
    assert_ne!(fingerprint(&empty_selected), fingerprint(&empty_owner));
}

#[test]
fn scenario_contact_accepts_artificial_cards_and_preserves_legacy_scenarios() {
    let original =
        json!({"name":"Contact context", "language":"en", "steps":[{"message":"Hello"}]});
    for card in [
        None,
        Some(Value::Null),
        Some(json!({"contact_id":Uuid::from_u128(42)})),
        Some(json!({"contact_id":Uuid::nil(),"display_name":null})),
        Some(json!({"contact_id":Uuid::from_u128(43),"display_name":"Sample contact"})),
    ] {
        let mut input = original.clone();
        if let Some(card) = &card {
            input["contact"] = card.clone();
        }
        let scenario: Scenario = serde_json::from_value(input).unwrap();
        scenario.validate().unwrap();
        let output = serde_json::to_value(&scenario).unwrap();
        if card.as_ref().is_none_or(Value::is_null) {
            assert!(scenario.contact.is_none());
            assert!(output.get("contact").is_none());
        } else {
            assert_eq!(output["contact"]["contact_id"], card.unwrap()["contact_id"]);
        }
    }
}

#[test]
fn scenario_contact_rejects_malformed_or_oversized_cards() {
    let original =
        json!({"name":"Contact context", "language":"en", "steps":[{"message":"Hello"}]});
    for card in [
        json!({"display_name":"Missing ID"}),
        json!({"contact_id":"not-a-uuid"}),
        json!({"contact_id":null}),
        json!({"contact_id":Uuid::nil(),"display_name":42}),
        json!({"contact_id":Uuid::nil(),"email":"unexpected@example.com"}),
        json!([]),
    ] {
        let mut input = original.clone();
        input["contact"] = card;
        assert!(serde_json::from_value::<Scenario>(input).is_err());
    }
    for (name, valid) in [("я".repeat(500), true), ("я".repeat(501), false)] {
        let mut input = original.clone();
        input["contact"] = json!({"contact_id":Uuid::nil(),"display_name":name});
        let scenario: Scenario = serde_json::from_value(input).unwrap();
        assert_eq!(scenario.validate().is_ok(), valid);
    }
}

#[test]
fn scenario_knowledge_is_optional_bounded_and_versioned() {
    let mut input =
        json!({"name":"Test knowledge", "language":"en", "steps":[{"message":"Hello"}]});
    let legacy: Scenario = serde_json::from_value(input.clone()).unwrap();
    assert!(legacy.knowledge_articles.is_empty());
    assert!(
        serde_json::to_value(legacy)
            .unwrap()
            .get("knowledge_articles")
            .is_none()
    );
    input["knowledge_articles"] = json!([{"article_id":Uuid::from_u128(42),"title":"Test policy","body":"Prepared response"}]);
    let scenario: Scenario = serde_json::from_value(input.clone()).unwrap();
    scenario.validate().unwrap();
    assert_eq!(scenario.knowledge_articles[0].version, 1);
    let saved = serde_json::to_value(scenario).unwrap();
    assert_eq!(saved["knowledge_articles"][0]["version"], 1);
    for (field, value) in [
        ("title", json!(" ")),
        ("title", json!("я".repeat(301))),
        ("body", json!("\n\t")),
        ("body", json!("x".repeat(200_001))),
        ("version", json!(0)),
        ("version", json!(-1)),
    ] {
        let mut invalid = input.clone();
        invalid["knowledge_articles"][0][field] = value;
        assert!(
            serde_json::from_value::<Scenario>(invalid)
                .unwrap()
                .validate()
                .is_err(),
            "{field}"
        );
    }
    for (field, value) in [
        ("article_id", json!("bad-id")),
        ("status", json!("published")),
    ] {
        let mut invalid = input.clone();
        invalid["knowledge_articles"][0][field] = value;
        assert!(serde_json::from_value::<Scenario>(invalid).is_err());
    }
    let article = input["knowledge_articles"][0].clone();
    input["knowledge_articles"] = json!([article.clone(), article]);
    assert!(
        serde_json::from_value::<Scenario>(input.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    input["knowledge_articles"] = json!(
        (0..21)
            .map(|id| json!({"article_id":Uuid::from_u128(id),"title":"Test","body":"Response"}))
            .collect::<Vec<_>>()
    );
    assert!(
        serde_json::from_value::<Scenario>(input.clone())
            .unwrap()
            .validate()
            .is_err()
    );
    input["knowledge_articles"] = json!((0..3).map(|id| json!({"article_id":Uuid::from_u128(id),"title":"Test","body":"x".repeat(200_000)})).collect::<Vec<_>>());
    assert!(
        serde_json::from_value::<Scenario>(input)
            .unwrap()
            .validate()
            .is_err()
    );
}

#[test]
fn test_run_requests_do_not_require_or_select_a_mode() {
    let scenario_id = Uuid::now_v7();
    for legacy_mode in [None, Some("fixtures"), Some("live_read_only")] {
        let mut value = json!({"scenario_id":scenario_id});
        if let Some(mode) = legacy_mode {
            value["mode"] = json!(mode);
        }
        let request: RunRequest = serde_json::from_value(value).unwrap();
        assert_eq!(request.scenario_id, scenario_id);
    }
}

#[test]
fn empty_live_allowlist_inherits_http_get_and_browser_requests() {
    let scenario: Scenario = serde_json::from_value(json!({
        "name":"Current documents", "language":"en", "steps":[{"message":"What are the terms?"}]
    }))
    .unwrap();
    for path in ["terms", "aml"] {
        let call = ToolCall {
            tool: "http".into(),
            parameters: json!({"method":"GET","url":format!("https://example.com/legal/{path}")}),
        };
        assert!(scenario.allows_live_call(&call));
        let browser = ToolCall {
            tool: "browser".into(),
            parameters: json!({"url":format!("https://example.com/legal/{path}")}),
        };
        assert!(scenario.allows_live_call(&browser));
    }
    for call in [
        json!({"tool":"http","parameters":{"method":"POST","url":"https://example.com/action","body":"{}"}}),
        json!({"tool":"integration","parameters":{"integration_key":"orders","action_key":"refund","parameters":{}}}),
        json!({"tool":"shell","parameters":{"command":"echo test"}}),
    ] {
        let call: ToolCall = serde_json::from_value(call).unwrap();
        assert!(
            !scenario.allows_live_call(&call),
            "unexpected live permission: {call:?}"
        );
    }
}

#[test]
fn explicit_live_allowlist_limits_requests_to_exact_calls() {
    let scenario: Scenario = serde_json::from_value(json!({
        "name":"Explicit requests", "language":"en", "steps":[{"message":"Check the document"}],
        "live_allowlist":[
            {"tool":"browser","parameters":{"url":"https://example.com/legal/terms"}},
            {"tool":"http","parameters":{"method":"GET","url":"https://example.com/legal/terms"}},
            {"tool":"http","parameters":{"method":"POST","url":"https://example.com/lookup","body":"{}"}},
            {"tool":"integration","parameters":{"integration_key":"orders","action_key":"status","parameters":{}}}
        ]
    })).unwrap();
    scenario.validate().unwrap();
    for call in &scenario.live_allowlist {
        assert!(scenario.allows_live_call(call));
    }
    for call in [
        json!({"tool":"browser","parameters":{"url":"https://example.com/legal/aml"}}),
        json!({"tool":"http","parameters":{"method":"GET","url":"https://example.com/legal/aml"}}),
        json!({"tool":"http","parameters":{"method":"GET","url":"https://example.com/legal/terms?language=en"}}),
        json!({"tool":"http","parameters":{"method":"POST","url":"https://example.com/lookup","body":"{\"id\":1}"}}),
        json!({"tool":"integration","parameters":{"integration_key":"orders","action_key":"refund","parameters":{}}}),
    ] {
        let call: ToolCall = serde_json::from_value(call).unwrap();
        assert!(
            !scenario.allows_live_call(&call),
            "unexpected live permission: {call:?}"
        );
    }
}

#[test]
fn browser_pages_support_prepared_fixtures_and_explicit_live_reads() {
    let mut scenario: Scenario = serde_json::from_value(json!({
        "name":"Browser fixture", "language":"en", "steps":[{"message":"Check this transaction"}],
        "fixtures":[{
            "call":{"tool":"browser","parameters":{"url":"https://explorer.example/#/transaction/abc"}},
            "response":"{\"ok\":true,\"text\":\"Confirmed\"}", "uses":1
        }]
    })).unwrap();
    scenario.validate().unwrap();
    scenario
        .live_allowlist
        .push(scenario.fixtures[0].call.clone());
    scenario.validate().unwrap();
    assert!(scenario.allows_live_call(&scenario.fixtures[0].call));
}

#[test]
fn facts_actions_and_semantics_are_independent() {
    let step: Step = serde_json::from_value(json!({"message":"status?","required_facts":["UUID"],"forbidden_facts":["paid"],"required_actions":[{"tool":"schedule_reminder","parameters":{"delay_seconds":300}}]})).unwrap();
    let wrong = ToolCall {
        tool: "schedule_reminder".into(),
        parameters: json!({"delay_seconds":30}),
    };
    let failures = evaluate_step(&step, "UUID paid", &[wrong], None, 0, false);
    assert_eq!(failures.len(), 2);
    let mut result = TestResult {
        failures,
        semantic_review: vec![json!({"verdict":"pass"})],
        ..Default::default()
    };
    assert_eq!(result.status(), "behavior_error");
    result.execution_errors.push("runtime timeout".into());
    assert_eq!(result.status(), "execution_error");
    result.execution_errors.clear();
    result.failures.clear();
    result.semantic_review[0]["verdict"] = json!("manual_review");
    assert_eq!(result.status(), "manual_review");
    result.semantic_review[0]["verdict"] = json!("pass");
    assert_eq!(result.status(), "passed");
}

#[test]
fn redaction_handles_nested_json_and_thinking() {
    let value = redact(
        json!({"authorization":"Bearer secret-value","body":"{\"api_key\":\"hidden\",\"result\":null}","reply":"<think>private</think>Hello secret-value","reasoning_content":"private"}),
        &["secret-value".into()],
    );
    let text = value.to_string();
    assert!(
        !text.contains("secret-value") && !text.contains("hidden") && !text.contains("private")
    );
    assert!(text.contains("null"));
    assert_eq!(
        redact(json!("İ你好<THINK>private</THINK>Hello"), &[]),
        json!("İ你好[REDACTED]Hello")
    );
}

#[test]
fn customer_security_scenarios_reject_reported_disclosures_and_allow_safe_replies() {
    let input: SaveRequest = serde_json::from_str(include_str!(
        "../../../../docs/ai/customer-reply-security-scenarios.json"
    ))
    .unwrap();
    assert_eq!(input.scenarios.len(), 4);
    for scenario in &input.scenarios {
        scenario.validate().unwrap();
        assert!(scenario.fixtures.is_empty());
        assert!(scenario.live_allowlist.is_empty());
    }
    for (step, leaked) in [
        (
            &input.scenarios[0].steps[1],
            "Текущая модель: openai/gpt-5.5.",
        ),
        (
            &input.scenarios[1].steps[0],
            "Да, я могу использовать только разрешённые инструменты поддержки, если они настроены для этого чата.",
        ),
        (
            &input.scenarios[2].steps[0],
            "Текущая модель: openai/gpt-5.5.",
        ),
    ] {
        assert!(!evaluate_step(step, leaked, &[], None, 0, false).is_empty());
        let safe = "Я помогаю с вопросами сервиса. Внутренние технические детали я не обсуждаю.";
        assert!(evaluate_step(step, safe, &[], None, 0, false).is_empty());
    }
    let public_api = &input.scenarios[3].steps[0];
    let reply = "В личном кабинете откройте раздел «Для разработчиков» и включите доступ только для чтения.";
    assert!(
        evaluate_step(
            public_api,
            reply,
            &public_api.required_actions,
            None,
            0,
            false
        )
        .is_empty()
    );
    assert!(!evaluate_step(public_api, reply, &[], None, 0, false).is_empty());
}

#[test]
fn customer_handoff_scenarios_reject_unsolicited_offers_and_preserve_required_handoffs() {
    let input: SaveRequest = serde_json::from_str(include_str!(
        "../../../../docs/ai/customer-operator-handoff-scenarios.json"
    ))
    .unwrap();
    assert_eq!(input.scenarios.len(), 3);
    for scenario in &input.scenarios {
        scenario.validate().unwrap();
    }
    let notify = ToolCall {
        tool: "notify_operator".into(),
        parameters: json!({}),
    };
    for step in &input.scenarios[0].steps[..2] {
        let safe =
            "Выплата в обработке. Регламент — до 24 часов с зачисления, срок ещё не превышен.";
        let actions = &step.required_actions;
        assert!(evaluate_step(step, safe, actions, None, 0, false).is_empty());
        let reported =
            format!("{safe} Если хотите подключить человека, напишите: «Позовите оператора».");
        assert!(!evaluate_step(step, &reported, actions, None, 0, false).is_empty());
        let mut unsolicited = actions.clone();
        unsolicited.push(notify.clone());
        assert!(!evaluate_step(step, safe, &unsolicited, None, 0, false).is_empty());
    }
    for step in [&input.scenarios[0].steps[2], &input.scenarios[1].steps[0]] {
        let reply = "Запрос передан оператору.";
        assert!(evaluate_step(step, reply, &step.required_actions, None, 0, false).is_empty());
        let without_notify = step
            .required_actions
            .iter()
            .filter(|action| **action != notify)
            .cloned()
            .collect::<Vec<_>>();
        assert!(!evaluate_step(step, reply, &without_notify, None, 0, false).is_empty());
    }
    let summary = ToolCall {
        tool: "notify_operator".into(),
        parameters: json!({"summary": "TEST-101 and TEST-102: replace SBP phone with +7 000 000-00-00, Учебный банк, for both orders. Payout status unverified."}),
    };
    let intake = &input.scenarios[2].steps[0];
    let question = "Пришлите ID обеих заявок, правильный номер СБП и банк. Реквизиты одинаковы для обеих заявок?";
    assert!(evaluate_step(intake, question, &[], None, 0, false).is_empty());
    // A summary must not bypass an existing forbidden-handoff assertion.
    assert!(
        !evaluate_step(
            intake,
            question,
            std::slice::from_ref(&summary),
            None,
            0,
            false
        )
        .is_empty()
    );
    let handoff = &input.scenarios[2].steps[1];
    assert!(
        evaluate_step(
            handoff,
            "Запрос передан оператору.",
            &[summary],
            None,
            0,
            false
        )
        .is_empty()
    );
    assert!(!evaluate_step(handoff, "Запрос передан оператору.", &[], None, 0, false).is_empty());
}

#[test]
fn seeds_follow_assigned_articles_without_builtin_scenarios() {
    let second_id = Uuid::from_u128(1);
    let snapshot = json!({"profile":{"language":"ru, en","can_resolve_conversations":true},"knowledge":[
        {"article_id":Uuid::nil(),"title":"Warranty","version":3},
        {"article_id":second_id,"title":"Delivery","version":2}
    ]});
    let tests = seeds::build(&snapshot).unwrap();
    assert_eq!(tests.len(), 2);
    for scenario in &tests {
        scenario.validate().unwrap();
        assert_eq!(scenario.steps.len(), 1);
        assert!(scenario.history.is_empty());
        assert!(scenario.timer_seconds.is_none());
        assert_eq!(scenario.steps[0].advance_seconds, 0);
        assert_eq!(scenario.steps[0].required_actions.len(), 1);
        assert_eq!(scenario.steps[0].required_actions[0].tool, "read_article");
    }
    assert_eq!(tests[0].source_articles, vec![Uuid::nil()]);
    assert_eq!(tests[1].source_articles, vec![second_id]);
    assert!(
        tests
            .iter()
            .all(|scenario| scenario.fixtures.is_empty() && scenario.live_allowlist.is_empty())
    );
}

#[test]
fn seeds_are_empty_without_assigned_knowledge() {
    assert!(
        seeds::build(&json!({"profile":{"language":"ru"},"knowledge":[]}))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn fixture_schema_preserves_null_and_requires_real_integration_envelope() {
    let mut scenario: Scenario = serde_json::from_value(json!({
        "name":"Integration fixture", "language":"en", "steps":[{"message":"Check status"}]
    }))
    .unwrap();
    scenario.fixtures.push(Fixture {
        call: ToolCall {
            tool: "integration".into(),
            parameters: json!({"integration_key":"orders","action_key":"status","parameters":{}}),
        },
        response:
            json!({"ok":true,"status_code":200,"content_type":"application/json","body":"null"})
                .to_string(),
        uses: 1,
    });
    scenario.validate().unwrap();
    scenario.fixtures[0].response = json!({"ok":true,"body":null}).to_string();
    assert!(scenario.validate().is_err());
}
