//! JSON Schema rewriting shared by provider strict modes.

use serde_json::{Map, Value, json};

use crate::JsonValue;

/// Rewrites a JSON Schema into the constrained subset that OpenAI-compatible
/// strict modes require, used both for strict tool parameters and for
/// `json_schema` response formats.
///
/// Every object is closed with `additionalProperties: false`, every property is
/// listed in `required`, originally-optional properties are made nullable, and
/// unsupported size keywords (`minLength`, `maxLength`, `minItems`, `maxItems`)
/// are dropped. The rewrite recurses through every place a subschema can appear
/// — `properties`, array `items`, the `$defs`/`definitions` that named types
/// reference, and the `anyOf`/`oneOf`/`allOf`/`prefixItems` composition
/// keywords — so a schema derived from a nested type is fully constrained, not
/// just its root.
pub fn strict_schema(schema: &JsonValue) -> JsonValue {
    let Value::Object(object) = schema else {
        return schema.clone();
    };

    let mut object = object.clone();
    for keyword in ["minLength", "maxLength", "minItems", "maxItems"] {
        object.remove(keyword);
    }

    if let Some(items) = object.get("items") {
        let rewritten = strict_schema(items);
        object.insert("items".to_owned(), rewritten);
    }

    for keyword in ["anyOf", "oneOf", "allOf", "prefixItems"] {
        if let Some(Value::Array(branches)) = object.get(keyword) {
            let rewritten: Vec<Value> = branches.iter().map(strict_schema).collect();
            object.insert(keyword.to_owned(), Value::Array(rewritten));
        }
    }

    for keyword in ["$defs", "definitions"] {
        if let Some(Value::Object(definitions)) = object.get(keyword) {
            let rewritten: Map<String, Value> = definitions
                .iter()
                .map(|(name, schema)| (name.clone(), strict_schema(schema)))
                .collect();
            object.insert(keyword.to_owned(), Value::Object(rewritten));
        }
    }

    if let Some(Value::Object(properties)) = object.remove("properties") {
        let required: Vec<String> = object
            .get("required")
            .and_then(Value::as_array)
            .map(|values| {
                values
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();

        let mut rewritten = Map::new();
        for (name, property) in properties {
            let mut property = strict_schema(&property);
            if !required.contains(&name) {
                property = make_nullable(property);
            }
            rewritten.insert(name, property);
        }

        let names: Vec<Value> = rewritten.keys().cloned().map(Value::String).collect();
        object.insert("properties".to_owned(), Value::Object(rewritten));
        object.insert("required".to_owned(), Value::Array(names));
        object.insert("additionalProperties".to_owned(), Value::Bool(false));
    }

    Value::Object(object)
}

/// Makes a property schema accept `null` without changing its other constraints.
fn make_nullable(schema: Value) -> Value {
    let Value::Object(mut object) = schema else {
        return schema;
    };

    match object.get("type").cloned() {
        Some(Value::String(single)) => {
            if single != "null" {
                object.insert("type".to_owned(), json!([single, "null"]));
            }
        }
        Some(Value::Array(mut variants)) => {
            if !variants.iter().any(|value| value == "null") {
                variants.push(Value::String("null".to_owned()));
            }
            object.insert("type".to_owned(), Value::Array(variants));
        }
        _ => {
            return json!({ "anyOf": [Value::Object(object), { "type": "null" }] });
        }
    }

    Value::Object(object)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn closes_objects_and_requires_every_property() {
        let rewritten = strict_schema(&json!({
            "type": "object",
            "properties": {
                "city": { "type": "string", "minLength": 1 },
                "note": { "type": "string" },
            },
            "required": ["city"],
        }));

        assert_eq!(rewritten["additionalProperties"], json!(false));
        assert_eq!(rewritten["required"], json!(["city", "note"]));
        assert!(rewritten["properties"]["city"].get("minLength").is_none());
        assert_eq!(rewritten["properties"]["city"]["type"], json!("string"));
        assert_eq!(
            rewritten["properties"]["note"]["type"],
            json!(["string", "null"])
        );
    }

    #[test]
    fn recurses_into_definitions_referenced_by_ref() {
        // The shape `schemars` emits for a nested type: the field is a `$ref`
        // into `$defs`, and the definition itself carries an optional property.
        let rewritten = strict_schema(&json!({
            "type": "object",
            "properties": { "address": { "$ref": "#/$defs/Address" } },
            "required": ["address"],
            "$defs": {
                "Address": {
                    "type": "object",
                    "properties": {
                        "street": { "type": "string" },
                        "zip": { "type": ["string", "null"] },
                    },
                    "required": ["street"],
                },
            },
        }));

        let address = &rewritten["$defs"]["Address"];
        assert_eq!(address["additionalProperties"], json!(false));
        assert_eq!(address["required"], json!(["street", "zip"]));
        assert_eq!(
            address["properties"]["zip"]["type"],
            json!(["string", "null"])
        );
        // The `$ref` itself is left intact; providers resolve it.
        assert_eq!(
            rewritten["properties"]["address"],
            json!({ "$ref": "#/$defs/Address" })
        );
    }

    #[test]
    fn recurses_into_composition_branches() {
        let rewritten = strict_schema(&json!({
            "anyOf": [
                {
                    "type": "object",
                    "properties": { "a": { "type": "integer" } },
                    "required": ["a"],
                },
                { "type": "null" },
            ],
        }));

        let first = &rewritten["anyOf"][0];
        assert_eq!(first["additionalProperties"], json!(false));
        assert_eq!(first["required"], json!(["a"]));
        assert_eq!(rewritten["anyOf"][1], json!({ "type": "null" }));
    }

    #[test]
    fn non_object_schema_passes_through() {
        assert_eq!(strict_schema(&json!(true)), json!(true));
        assert_eq!(strict_schema(&json!("text")), json!("text"));
    }
}
