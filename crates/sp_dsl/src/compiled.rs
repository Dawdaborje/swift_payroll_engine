use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use cel::{Context, Program, Value};
use once_cell::sync::Lazy;
use rhai::{AST, Engine as RhaiEngine, Scope};
use rust_decimal::Decimal;
use rust_decimal::prelude::{FromPrimitive, ToPrimitive};

use crate::models::{DslType, PayrollRuleError};

static CEL_INT_RE: Lazy<regex::Regex> =
    Lazy::new(|| regex::Regex::new(r"\b\d+\b").expect("cel int regex"));

thread_local! {
    static CEL_PROGRAM_CACHE: RefCell<HashMap<String, Program>> = RefCell::new(HashMap::new());
    static RHAI_AST_CACHE: RefCell<HashMap<String, AST>> = RefCell::new(HashMap::new());
}

/// Cached rule source. CEL `Program` and Rhai `AST` are compiled once per OS thread.
#[derive(Clone)]
pub enum CompiledRule {
    Cel(String),
    Rhai(String),
}

impl CompiledRule {
    pub fn compile(dsl_type: DslType, expression: &str) -> Result<Self, PayrollRuleError> {
        match dsl_type {
            DslType::CEL => {
                // Validate up front so bad rules fail at load, not first payroll run.
                let source = normalize_cel_integers(expression);
                Program::compile(&source)
                    .map_err(|e| PayrollRuleError::Parse(format!("CEL parse error: {e}")))?;
                Ok(CompiledRule::Cel(expression.to_string()))
            }
            DslType::Rhai => {
                let engine = RhaiEngine::new();
                engine
                    .compile(expression)
                    .map_err(|e| PayrollRuleError::Parse(format!("Rhai parse error: {e}")))?;
                Ok(CompiledRule::Rhai(expression.to_string()))
            }
            DslType::OdooHRMS => Err(PayrollRuleError::UnsupportedDsl("OdooHRMS".into())),
            DslType::FrappeHRMS => Err(PayrollRuleError::UnsupportedDsl("FrappeHRMS".into())),
        }
    }

    pub fn evaluate(&self, gross: Decimal) -> Result<Decimal, PayrollRuleError> {
        let gross_f64 = gross.to_f64().unwrap_or(0.0);
        match self {
            CompiledRule::Cel(expression) => eval_cel(expression, gross_f64),
            CompiledRule::Rhai(expression) => eval_rhai(expression, gross_f64),
        }
    }
}

fn eval_cel(expression: &str, gross_f64: f64) -> Result<Decimal, PayrollRuleError> {
    CEL_PROGRAM_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(expression) {
            let source = normalize_cel_integers(expression);
            let program = Program::compile(&source)
                .map_err(|e| PayrollRuleError::Parse(format!("CEL parse error: {e}")))?;
            cache.insert(expression.to_string(), program);
        }
        let program = cache.get(expression).expect("cel program cache insert");

        let mut ctx = Context::default();
        ctx.add_variable("gross", gross_f64)
            .map_err(|e| PayrollRuleError::Evaluation(format!("CEL context: {e}")))?;
        let value = program
            .execute(&ctx)
            .map_err(|e| PayrollRuleError::Evaluation(format!("CEL evaluation: {e}")))?;
        cel_value_to_decimal(value)
    })
}

fn eval_rhai(expression: &str, gross_f64: f64) -> Result<Decimal, PayrollRuleError> {
    RHAI_AST_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(expression) {
            let engine = RhaiEngine::new();
            let ast = engine
                .compile(expression)
                .map_err(|e| PayrollRuleError::Parse(format!("Rhai parse error: {e}")))?;
            cache.insert(expression.to_string(), ast);
        }
        let ast = cache.get(expression).expect("rhai ast cache insert");

        let engine = RhaiEngine::new();
        let mut scope = Scope::new();
        scope.push("gross", gross_f64);
        let val = engine
            .eval_ast_with_scope::<rhai::Dynamic>(&mut scope, ast)
            .map_err(|e| PayrollRuleError::Evaluation(format!("Rhai evaluation: {e}")))?;
        rhai_value_to_decimal(val)
    })
}

fn cel_value_to_decimal(value: Value) -> Result<Decimal, PayrollRuleError> {
    let f = match value {
        Value::Float(f) => f,
        Value::Int(i) => i as f64,
        _ => {
            return Err(PayrollRuleError::Evaluation(
                "CEL expression did not evaluate to a number".into(),
            ));
        }
    };
    Decimal::from_f64(f).ok_or_else(|| {
        PayrollRuleError::Evaluation("CEL result is not a valid decimal".into())
    })
}

fn rhai_value_to_decimal(val: rhai::Dynamic) -> Result<Decimal, PayrollRuleError> {
    if let Ok(i) = val.as_int() {
        Decimal::from_i64(i).ok_or_else(|| {
            PayrollRuleError::Evaluation("Rhai integer result out of range".into())
        })
    } else if let Ok(f) = val.as_float() {
        Decimal::from_f64(f).ok_or_else(|| {
            PayrollRuleError::Evaluation("Rhai float result is not a valid decimal".into())
        })
    } else {
        Err(PayrollRuleError::Evaluation(
            "Rhai expression did not evaluate to a number".into(),
        ))
    }
}

fn normalize_cel_integers(expression: &str) -> String {
    CEL_INT_RE
        .replace_all(expression, |caps: &regex::Captures| {
            let m = caps.get(0).unwrap();
            let start = m.start();
            let end = m.end();
            let bytes = expression.as_bytes();
            let preceded_by_dot = start > 0 && bytes[start - 1] == b'.';
            let followed_by_dot = end < bytes.len() && bytes[end] == b'.';
            if preceded_by_dot || followed_by_dot {
                m.as_str().to_string()
            } else {
                format!("{}.0", m.as_str())
            }
        })
        .into_owned()
}

#[derive(Hash, PartialEq, Eq, Clone)]
struct CacheKey {
    dsl_type: DslType,
    expression: String,
}

/// Global dedup of rule sources. Per-thread program/AST caches live in `CompiledRule::evaluate`.
pub struct RuleCache {
    entries: RwLock<HashMap<CacheKey, Arc<CompiledRule>>>,
}

impl RuleCache {
    pub fn new() -> Self {
        Self {
            entries: RwLock::new(HashMap::new()),
        }
    }

    pub fn global() -> &'static RuleCache {
        static CACHE: Lazy<RuleCache> = Lazy::new(RuleCache::new);
        &CACHE
    }

    pub fn get_or_compile(
        &self,
        dsl_type: DslType,
        expression: &str,
    ) -> Result<Arc<CompiledRule>, PayrollRuleError> {
        let key = CacheKey {
            dsl_type: dsl_type.clone(),
            expression: expression.to_string(),
        };
        if let Some(hit) = self.entries.read().expect("cache read lock").get(&key) {
            return Ok(Arc::clone(hit));
        }
        let compiled = Arc::new(CompiledRule::compile(dsl_type, expression)?);
        let mut guard = self.entries.write().expect("cache write lock");
        let entry = guard.entry(key).or_insert_with(|| Arc::clone(&compiled));
        Ok(Arc::clone(entry))
    }

    pub fn len(&self) -> usize {
        self.entries.read().expect("cache read lock").len()
    }
}

impl Default for RuleCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cache_deduplicates_identical_expressions() {
        let cache = RuleCache::new();
        let a = cache
            .get_or_compile(DslType::CEL, "gross * 0.05")
            .expect("compile");
        let b = cache
            .get_or_compile(DslType::CEL, "gross * 0.05")
            .expect("compile");
        assert!(Arc::ptr_eq(&a, &b));
        assert_eq!(cache.len(), 1);
    }

    #[test]
    fn compiled_cel_matches_rate() {
        let rule = CompiledRule::compile(DslType::CEL, "gross * 0.10").unwrap();
        let amount = rule.evaluate(Decimal::new(20000, 0)).unwrap();
        assert_eq!(amount, Decimal::new(2000, 0));
    }

    #[test]
    fn compiled_rhai_matches_rate() {
        let rule = CompiledRule::compile(DslType::Rhai, "gross * 0.10").unwrap();
        let amount = rule.evaluate(Decimal::new(20000, 0)).unwrap();
        assert_eq!(amount, Decimal::new(2000, 0));
    }
}
