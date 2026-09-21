use super::{Context, Result, Scenario, Step, ToolCall, Value, json};

fn step(message: &str, meaning: &str) -> Step {
    serde_json::from_value(json!({"message":message,"expected_meaning":meaning}))
        .expect("static test step")
}

fn scenario(name: &str, question: &str, meaning: &str) -> Scenario {
    Scenario {
        name: name.into(),
        language: "ru".into(),
        channel_id: None,
        contact: None,
        knowledge_articles: Vec::new(),
        history: Vec::new(),
        operator_present: false,
        closed: false,
        timer_seconds: None,
        steps: vec![step(question, meaning)],
        fixtures: Vec::new(),
        live_allowlist: Vec::new(),
        source_articles: Vec::new(),
    }
}

pub fn build(snapshot: &Value) -> Result<Vec<Scenario>> {
    let mut tests = Vec::new();
    // Coverage follows the current assignment. Article text remains in the versioned KB snapshot,
    // and is never copied into the customer question or a bulk knowledge prompt.
    for article in snapshot["knowledge"]
        .as_array()
        .context("knowledge unavailable")?
    {
        let title = article["title"].as_str().unwrap_or_default();
        let mut test = scenario(
            &format!("База знаний: {title}"),
            &format!("Объясните, пожалуйста: {title}. Какие условия мне нужно знать?"),
            "Ответ опирается на актуальную опубликованную статью. Условия и числа не добавлены от себя. Примеры не трактуются как факты о текущем обращении.",
        );
        test.source_articles
            .push(serde_json::from_value(article["article_id"].clone())?);
        test.steps[0].required_actions.push(ToolCall{tool:"read_article".into(),parameters:json!({"article_id":article["article_id"],"offset":0,"version":article["version"]})});
        tests.push(test);
    }
    Ok(tests)
}
