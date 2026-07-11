use crate::models::deduction::EmpDeduction;

use super::models::emp_context::EmployeeContext;

use rayon::prelude::*;
use sp_dsl::models::PayrollRuleError;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum EngineError {
    #[error("calculation failed: {0}")]
    CalculationError(String),

    #[error("DSL evaluation failed for employee {employee_id}, rule {rule}: {source}")]
    DslEvaluation {
        employee_id: String,
        rule: String,
        #[source]
        source: PayrollRuleError,
    },

    #[error("invalid input: {0}")]
    InvalidInput(String),
}

pub struct CalculationContext {
    pub emp_contexts: Vec<EmployeeContext>,
}

#[derive(Debug)]
pub struct CalculationResult {
    pub employee_id: String,
    pub gross_salary: rust_decimal::Decimal,
    pub net_salary: rust_decimal::Decimal,
    pub deductions: Vec<EmpDeduction>,
}

impl CalculationContext {
    pub fn calculate(&self) -> Result<Vec<CalculationResult>, EngineError> {
        self.emp_contexts
            .par_iter()
            .map(Self::calculate_one)
            .collect()
    }

    pub fn calculate_sequential(&self) -> Result<Vec<CalculationResult>, EngineError> {
        self.emp_contexts.iter().map(Self::calculate_one).collect()
    }

    pub fn calculate_stream<F>(count: usize, at: F) -> Result<Vec<CalculationResult>, EngineError>
    where
        F: Fn(usize) -> EmployeeContext + Sync + Send,
    {
        (0..count)
            .into_par_iter()
            .map(|i| Self::calculate_one(&at(i)))
            .collect()
    }

    pub fn calculate_stream_count<F>(count: usize, at: F) -> Result<usize, EngineError>
    where
        F: Fn(usize) -> EmployeeContext + Sync + Send,
    {
        (0..count)
            .into_par_iter()
            .try_for_each(|i| Self::calculate_one(&at(i)).map(|_| ()))?;
        Ok(count)
    }

    pub fn calculate_stream_sequential_count<F>(
        count: usize,
        at: F,
    ) -> Result<usize, EngineError>
    where
        F: Fn(usize) -> EmployeeContext,
    {
        for i in 0..count {
            Self::calculate_one(&at(i))?;
        }
        Ok(count)
    }

    fn calculate_one(emp_context: &EmployeeContext) -> Result<CalculationResult, EngineError> {
        if emp_context.base_salary.is_sign_negative() {
            return Err(EngineError::InvalidInput(format!(
                "employee {} has negative base_salary",
                emp_context.identity.emp_id
            )));
        }
        for allowance in &emp_context.allowances {
            if allowance.amount.is_sign_negative() {
                return Err(EngineError::InvalidInput(format!(
                    "employee {} has negative allowance {}",
                    emp_context.identity.emp_id, allowance.name
                )));
            }
        }

        let mut gross_salary = emp_context.calculate_gross().round_dp_with_strategy(
            2,
            rust_decimal::RoundingStrategy::MidpointAwayFromZero,
        );
        gross_salary.rescale(2);

        let mut deductions = emp_context.deductions.clone();
        let mut total_deduction = rust_decimal::Decimal::new(0, 0);

        for d in deductions.iter_mut() {
            if let Some(rule) = &d.dsl_expr {
                let amount =
                    rule.evaluate(gross_salary)
                        .map_err(|source| EngineError::DslEvaluation {
                            employee_id: emp_context.identity.emp_id.clone(),
                            rule: rule.rule_id.clone(),
                            source,
                        })?;
                d.amount = amount;
            }
            d.amount.rescale(2);
            total_deduction += d.amount;
        }

        let mut net_salary = gross_salary
            - total_deduction.round_dp_with_strategy(
                2,
                rust_decimal::RoundingStrategy::MidpointAwayFromZero,
            );
        net_salary.rescale(2);

        Ok(CalculationResult {
            employee_id: emp_context.identity.emp_id.clone(),
            gross_salary,
            net_salary,
            deductions,
        })
    }

    pub fn evaluate_deductions() {}
}
