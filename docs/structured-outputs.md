# Structured Outputs

## What are structured outputs?
Constraining a model reply to a known shape so it parses back into a Rust type.

## `ResponseFormat`
The enum and its three variants: `Text` (default), `JsonObject`, `JsonSchema`.

### JSON object mode
`JsonObject`: valid JSON with no schema, and the prompt requirement.

### JSON schema mode
`JsonSchema(JsonSchemaFormat)`: schema-constrained output.

## Deriving a schema from a type
`ResponseFormat::json_schema_for::<T>(name)` using `schemars` `JsonSchema`.

## Hand-building a schema
`ResponseFormat::json_schema(name, schema)` for schemas you construct yourself.

## `JsonSchemaFormat`
The record's fields (`name`, `description`, `schema`, `strict`) and its builder methods.

## Strict mode
Strict defaults to on; what the constrained-subset rewriter does and where it's shared.

### The constrained subset
`additionalProperties: false`, all properties `required`, optionals made nullable, size keywords dropped.

### Schema name rules
The `^[A-Za-z0-9_-]{1,64}$` constraint enforced before the request is sent.

## Parsing the response
Accumulating `TextDelta`s and deserializing into the target type.

## Provider support
Which providers accept `JsonSchema` natively (OpenAI, OpenRouter) and which reject it as `Config` (DeepSeek).
