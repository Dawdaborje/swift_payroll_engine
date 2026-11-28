use std::collections::BTreeMap;

use rust_decimal::Decimal;
use serde_json::Value as Json;
use swift_payroll_expr::Value;

/// JSON into an expression value. A JSON string stays text; a JSON number is read exactly as written
/// with the `exact-json` feature, and through an `f64` (exact to 15 significant digits) without it.
pub fn value_from_json(json: &Json) -> Result<Value, String> {
    Ok(match json {
        Json::Null => Value::Null,
        Json::Bool(b) => Value::Bool(*b),
        Json::Number(n) => {
            let text = n.to_string();
            Value::Num(
                Decimal::from_str_exact(&text)
                    .or_else(|_| Decimal::from_scientific(&text))
                    .map_err(|_| format!("`{text}` is not a number this engine can hold exactly"))?,
            )
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
        // The two crates are released together at the same version, so a new kind of value gets its own
        // arm here in the same release; text keeps it readable until then.
        other => Json::String(other.to_string()),
    }
}

/// Decimal text (`"120000.50"`, `"-3"`) as an exact number. Nothing else is accepted: no thousands
/// separators, no exponent, no currency sign.
pub(crate) fn number_from_text(text: &str) -> Result<Decimal, String> {
    let trimmed = text.trim();
    let plain = !trimmed.is_empty() && trimmed.trim_start_matches(['-', '+']).chars().all(|c| c.is_ascii_digit() || c == '.');
    if !plain {
        return Err(format!("`{text}` is not a decimal number"));
    }
    Decimal::from_str_exact(trimmed).map_err(|_| format!("`{text}` is not a decimal number this engine can hold exactly"))
}
