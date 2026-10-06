use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde_json::Value as Json;
use sp_expr::Value;

/// JSON into an expression value. A JSON number is read exactly as written; a JSON string stays text.
pub fn value_from_json(json: &Json) -> Result<Value, String> {
    Ok(match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => {
            let text = n.to_string();
            Value::Num(Decimal::from_str_exact(&text).or_else(|_| Decimal::from_scientific(&text)).map_err(|_| format!("`{text}` is not a number this engine can hold exactly"))?)
        }
        Json::String(s) => Value::Str(s.clone()),
        Json::Array(items) => Value::List(items.iter().map(value_from_json).collect::<Result<_, _>>()?),
        Json::Object(map) => Value::Map(map.iter().map(|(k, v)| Ok((k.clone(), value_from_json(v)?))).collect::<Result<BTreeMap<_, _>, String>>()?),
    })
}

/// An expression value as JSON; numbers become text so no digit is lost.
pub fn value_to_json(value: &Value) -> Json {
    match value {
        Value::Null => Json::Null,
        Value::Bool(b) => Json::Bool(*b),
        Value::Num(n) => Json::String(n.to_string()),
        Value::Str(s) => Json::String(s.clone()),
        Value::List(items) => Json::Array(items.iter().map(value_to_json).collect()),
        Value::Map(map) => Json::Object(map.iter().map(|(k, v)| (k.clone(), value_to_json(v))).collect()),
    }
}
