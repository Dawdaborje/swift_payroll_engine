use crate::engines::DslEngineImple;
use cel::{Context, Program, Value};
use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;

pub struct CelEngine {
    pub name: String,
    pub expression: String,
}

impl DslEngineImple for CelEngine {
    fn envaluate(&self, gross: rust_decimal::Decimal) -> super::DslEngineResult {
        // Automatically convert whole numbers to floats to avoid CEL evaluation errors
        let re = regex::Regex::new(r"\b\d+\b").unwrap();
        let expression_f64 = re.replace_all(&self.expression, |caps: &regex::Captures| {
            let m = caps.get(0).unwrap();
            let start = m.start();
            let end = m.end();
            let expr_bytes = self.expression.as_bytes();
            
            let preceded_by_dot = start > 0 && expr_bytes[start - 1] == b'.';
            let followed_by_dot = end < expr_bytes.len() && expr_bytes[end] == b'.';
            
            if preceded_by_dot || followed_by_dot {
                m.as_str().to_string()
            } else {
                format!("{}.0", m.as_str())
            }
        });

        match Program::compile(&expression_f64) {
            Ok(program) => {
                let mut ctx = Context::default();
                let _ = ctx.add_variable("gross", rust_decimal::prelude::ToPrimitive::to_f64(&gross).unwrap_or(0.0));

                match program.execute(&ctx) {
                    Ok(value) => {
                        let f_value = match value {
                            Value::Float(f) => f,
                            Value::Int(i) => i as f64,
                            _ => {
                                return super::DslEngineResult {
                                    success: false,
                                    message: None,
                                    error: Some(format!("CEL evaluation error")),
                                    name: self.name.clone(),
                                    value: None,
                                };
                            }
                        };
                        super::DslEngineResult {
                            success: true,
                            message: Some("CEL evaluation succeeded".to_string()),
                            error: None,
                            name: self.name.clone(),
                            value: Decimal::from_f64(f_value),
                        }
                    }
                    Err(e) => super::DslEngineResult {
                        success: false,
                        message: None,
                        error: Some(format!("CEL evaluation error: {e}")),
                        name: self.name.clone(),
                        value: None,
                    },
                }
            }
            Err(e) => super::DslEngineResult {
                success: false,
                message: None,
                error: Some(format!("CEL parse error: {e}")),
                name: self.name.clone(),
                value: None,
            },
        }
    }
}
