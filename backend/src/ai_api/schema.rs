//! Bounded JSON Schema inference and validation for saved API examples.

use serde_json::{Value, json};

const MAX_BYTES: usize = 64 * 1024;
const MAX_DEPTH: usize = 12;
const MAX_NODES: usize = 2_000;

pub(super) fn infer(example: &Value) -> Result<Value, String> {
    bounded_object(example)?;
    let mut nodes = 0;
    infer_value(example, 0, &mut nodes)
}

fn bounded_object(value: &Value) -> Result<(), String> {
    if !value.is_object() {
        return Err("the root must be a JSON object".into());
    }
    if value.to_string().len() > MAX_BYTES {
        return Err("JSON must not exceed 64 KiB".into());
    }
    Ok(())
}

fn check_bounds(depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if depth > MAX_DEPTH || *nodes > MAX_NODES {
        return Err("JSON exceeds the maximum depth or number of values".into());
    }
    Ok(())
}

fn infer_value(value: &Value, depth: usize, nodes: &mut usize) -> Result<Value, String> {
    check_bounds(depth, nodes)?;
    Ok(match value {
        Value::Object(object) => {
            let mut properties = serde_json::Map::new();
            for (key, child) in object {
                properties.insert(key.clone(), infer_value(child, depth + 1, nodes)?);
            }
            json!({"type":"object", "properties":properties,
                "required":object.keys().collect::<Vec<_>>(), "additionalProperties":false})
        }
        Value::Array(values) => {
            let first = values
                .first()
                .ok_or("example arrays must contain a typed item")?;
            let items = infer_value(first, depth + 1, nodes)?;
            for item in values.iter().skip(1) {
                let inferred = infer_value(item, depth + 1, nodes)?;
                if inferred != items {
                    return Err("every example array item must have the same schema".into());
                }
            }
            json!({"type":"array", "items":items})
        }
        Value::Bool(_) => json!({"type":"boolean"}),
        Value::String(_) => json!({"type":"string"}),
        Value::Number(_) => json!({"type":"number"}),
        Value::Null => json!({"type":"null"}),
    })
}

pub(super) fn validate(value: &Value, schema: &Value) -> Result<(), String> {
    bounded_object(value)?;
    let mut nodes = 0;
    validate_value(value, schema, "$", 0, &mut nodes)
}

fn validate_value(
    value: &Value,
    schema: &Value,
    path: &str,
    depth: usize,
    nodes: &mut usize,
) -> Result<(), String> {
    check_bounds(depth, nodes)?;
    let valid = match schema["type"].as_str() {
        Some("object") => {
            let object = value
                .as_object()
                .ok_or_else(|| format!("{path} must be an object"))?;
            let properties = schema["properties"]
                .as_object()
                .ok_or("invalid saved schema")?;
            if object.len() != properties.len()
                || object.keys().any(|key| !properties.contains_key(key))
            {
                return Err(format!("{path} must contain exactly the configured fields"));
            }
            for (key, child_schema) in properties {
                let child = object
                    .get(key)
                    .ok_or_else(|| format!("{path}.{key} is required"))?;
                validate_value(
                    child,
                    child_schema,
                    &format!("{path}.{key}"),
                    depth + 1,
                    nodes,
                )?;
            }
            true
        }
        Some("array") => {
            let items = value
                .as_array()
                .ok_or_else(|| format!("{path} must be an array"))?;
            for (index, item) in items.iter().enumerate() {
                validate_value(
                    item,
                    &schema["items"],
                    &format!("{path}[{index}]"),
                    depth + 1,
                    nodes,
                )?;
            }
            true
        }
        Some("boolean") => value.is_boolean(),
        Some("string") => value.is_string(),
        Some("number") => value.is_number(),
        Some("null") => value.is_null(),
        _ => false,
    };
    if valid {
        Ok(())
    } else {
        Err(format!("{path} has the wrong JSON type"))
    }
}

#[cfg(test)]
mod tests {
    use super::{infer, validate};
    use serde_json::json;

    #[test]
    fn strict_examples_reject_wrong_types_missing_and_extra_fields() {
        let schema = infer(&json!({"address_empty":true,"items":[{"balance":0}]})).unwrap();
        assert!(validate(&json!({"address_empty":false,"items":[]}), &schema).is_ok());
        for invalid in [
            json!({"address_empty":"false","items":[]}),
            json!({"address_empty":false}),
            json!({"address_empty":false,"items":[],"extra":1}),
            json!({"address_empty":false,"items":[{"balance":"0"}]}),
        ] {
            assert!(validate(&invalid, &schema).is_err());
        }
    }

    #[test]
    fn ambiguous_and_unbounded_examples_are_rejected() {
        for invalid in [
            json!([]),
            json!({"items":[]}),
            json!({"items":[true, "true"]}),
            json!({"text":"x".repeat(65536)}),
        ] {
            assert!(infer(&invalid).is_err());
        }
        let mut nested = json!(null);
        for _ in 0..14 {
            nested = json!({"nested":nested});
        }
        assert!(infer(&nested).is_err());
    }
}
