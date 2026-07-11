use sp_dsl::models::{DlsContext, DslType};
use thiserror::Error;

use crate::models::deduction::EmpDeduction;

#[derive(Debug, Error)]
pub enum DeductionError {
    #[error("payroll rule error")]
    PayrollRuleError,
    #[error("deduction rule error")]
    DeductionRuleError,
    #[error("dsl error")]
    DslError,
    #[error("no fixed amount")]
    NoFixedAmount,
}

pub struct DeductionRule {
    pub name: String,
    pub dsl: Option<String>,
    pub dsl_type: Option<DslType>,
    pub fixed_amount: Option<rust_decimal::Decimal>,
    pub rule_type: DeductionRuleType,
}

pub enum EmploymentType {
    FullTime,
    Contract,
    PartTime,
}

pub struct ComputedDeduction {
    pub id: String,
    pub label: String,
    pub amount: rust_decimal::Decimal,
}

#[derive(Debug, Clone)]
pub struct Allowance {
    pub name: String,
    pub amount: rust_decimal::Decimal,
}

#[derive(Debug, Clone)]
pub struct EmployeeIdentity {
    pub emp_id: String,
    pub name: Option<String>,
}

#[derive(Debug, Clone)]
pub struct EmployeeContext {
    pub identity: EmployeeIdentity,
    pub base_salary: rust_decimal::Decimal,
    pub allowances: Vec<Allowance>,
    pub deductions: Vec<EmpDeduction>,
    pub pay_period: Option<String>,
}

pub enum DeductionRuleType {
    Fixed,
    Dsl,
}

impl EmployeeContext {
    pub fn calculate_gross(&self) -> rust_decimal::Decimal {
        let mut total = self.base_salary;
        for allowance in &self.allowances {
            total += allowance.amount;
        }
        // Gross salary is base plus allowances; no double counting of base salary.
        total
    }

    pub fn calculate_net(&self, gross_salary: rust_decimal::Decimal) -> rust_decimal::Decimal {
        let deductions = &self.deductions;
        let net_salary = gross_salary
            - deductions
                .iter()
                .map(|d| d.amount)
                .sum::<rust_decimal::Decimal>();
        net_salary
    }

    pub fn build_deductions(
        &self,
        _dls_context: &DlsContext,
        rules: Vec<DeductionRule>,
    ) -> Result<Vec<ComputedDeduction>, DeductionError> {
        let mut deductions = Vec::new();

        for rule in &rules {
            match rule.rule_type {
                DeductionRuleType::Fixed => {
                    if let Some(amount) = rule.fixed_amount {
                        deductions.push(ComputedDeduction {
                            id: rule.name.clone(),
                            label: rule.name.clone(),
                            amount,
                        });
                    } else {
                        return Err(DeductionError::NoFixedAmount);
                    }
                }

                DeductionRuleType::Dsl => {
                    // TODO
                }
            }
        }

        Ok(deductions)
    }
}
