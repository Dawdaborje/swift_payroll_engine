use serde::{Deserialize, Serialize};
use std::sync::Arc;
use thiserror::Error;

use crate::compiled::{CompiledRule, RuleCache};

#[derive(Debug, Error)]
pub enum PayrollRuleError {
    #[error("invalid DSL type")]
    InvalidDslType,

    #[error("unsupported DSL type: {0}")]
    UnsupportedDsl(String),

    #[error("evaluation failed: {0}")]
    Evaluation(String),

    #[error("parse error: {0}")]
    Parse(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, Hash, PartialEq, Eq, Serialize, Deserialize)]
pub enum DslType {
    OdooHRMS,
    FrappeHRMS,
    Rhai,
    CEL,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlsContext {
    pub gross_salary: rust_decimal::Decimal,
}

#[derive(Clone, Serialize)]
pub struct PayrollRuleContext {
    pub rule_id: String,
    pub rule_name: String,
    pub dsl: String,
    pub dsl_type: DslType,
    #[serde(skip)]
    compiled: Arc<CompiledRule>,
}

impl std::fmt::Debug for PayrollRuleContext {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PayrollRuleContext")
            .field("rule_id", &self.rule_id)
            .field("rule_name", &self.rule_name)
            .field("dsl_type", &self.dsl_type)
            .field("dsl", &self.dsl)
            .finish_non_exhaustive()
    }
}

impl PayrollRuleContext {
    pub fn new(
        rule_id: String,
        rule_name: String,
        dsl: String,
        dsl_type: DslType,
    ) -> Result<Self, PayrollRuleError> {
        let compiled = RuleCache::global().get_or_compile(dsl_type.clone(), &dsl)?;
        Ok(Self {
            rule_id,
            rule_name,
            dsl,
            dsl_type,
            compiled,
        })
    }

    pub fn evaluate(
        &self,
        gross: rust_decimal::Decimal,
    ) -> Result<rust_decimal::Decimal, PayrollRuleError> {
        self.compiled.evaluate(gross)
    }
}
