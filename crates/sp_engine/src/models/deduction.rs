use sp_dsl::models::{DslType, PayrollRuleContext, PayrollRuleError};
use thiserror::Error;

#[derive(Debug, Clone)]
pub struct EmpDeduction {
    pub name: String,
    pub amount: rust_decimal::Decimal,
    pub dsl_expr: Option<PayrollRuleContext>,
}

#[derive(Debug, Error)]
pub enum EmpDeductionError {
    #[error("employee deduction error")]
    GeneralError,
    #[error("unsupported DSL type")]
    UnsupportedDsl,
    #[error("DSL deduction is missing a non-empty expression")]
    MissingExpression,
    #[error("fixed deduction is missing an amount")]
    MissingAmount,
    #[error("unknown deduction engine: {0}")]
    UnknownEngine(String),
    #[error("failed to compile DSL rule: {0}")]
    CompileRule(#[from] PayrollRuleError),
}

impl EmpDeduction {
    pub fn build_from_dsl(
        dsl_id: String,
        dsl_name: String,
        dsl_code: String,
        dsl_type: DslType,
    ) -> Result<Self, EmpDeductionError> {
        let payroll_rule = PayrollRuleContext::new(dsl_id.clone(), dsl_name, dsl_code, dsl_type)?;

        Ok(Self {
            name: dsl_id,
            amount: rust_decimal::Decimal::new(0, 0),
            dsl_expr: Some(payroll_rule),
        })
    }

    pub fn new(name: String, amount: rust_decimal::Decimal) -> Self {
        Self {
            name,
            amount,
            dsl_expr: None,
        }
    }
}
