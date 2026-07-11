use std::fs;
use std::path::{Path, PathBuf};

use sp_dsl::models::DslType;
use sp_engine::models::{
    deduction::{EmpDeduction, EmpDeductionError},
    emp_context::{Allowance, EmployeeContext, EmployeeIdentity},
};
use thiserror::Error;

use crate::cli::models::{RawAllowance, RawDeduction, RawEmployee};

#[derive(Debug, Error)]
pub enum FixtureError {
    #[error(transparent)]
    Io(#[from] std::io::Error),

    #[error(transparent)]
    Json(#[from] serde_json::Error),

    #[error(transparent)]
    Deduction(#[from] EmpDeductionError),

    #[error("fixture entry {0} is missing required `results` field")]
    MissingResults(String),
}

pub fn example_data_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../example_data")
}

pub fn load_employees(path: impl AsRef<Path>) -> Result<Vec<RawEmployee>, FixtureError> {
    let data = fs::read_to_string(path)?;
    Ok(serde_json::from_str(&data)?)
}

fn build_allowances(raw: &RawAllowance) -> Vec<Allowance> {
    vec![
        Allowance {
            name: "transport".to_string(),
            amount: raw.transport,
        },
        Allowance {
            name: "housing".to_string(),
            amount: raw.housing,
        },
        Allowance {
            name: "meal".to_string(),
            amount: raw.meal,
        },
    ]
}

pub fn build_deductions(raw: &[RawDeduction]) -> Result<Vec<EmpDeduction>, EmpDeductionError> {
    raw.iter()
        .map(|d| match d.engine.as_str() {
            "cel" => {
                let expression = d
                    .expression
                    .clone()
                    .filter(|e| !e.is_empty())
                    .ok_or(EmpDeductionError::MissingExpression)?;
                EmpDeduction::build_from_dsl(
                    d.id.clone(),
                    d.label.clone(),
                    expression,
                    DslType::CEL,
                )
            }
            "rhai" => {
                let expression = d
                    .expression
                    .clone()
                    .filter(|e| !e.is_empty())
                    .ok_or(EmpDeductionError::MissingExpression)?;
                EmpDeduction::build_from_dsl(
                    d.id.clone(),
                    d.label.clone(),
                    expression,
                    DslType::Rhai,
                )
            }
            "fixed" => {
                let amount = d.amount.ok_or(EmpDeductionError::MissingAmount)?;
                Ok(EmpDeduction::new(d.id.clone(), amount))
            }
            other => Err(EmpDeductionError::UnknownEngine(other.to_string())),
        })
        .collect()
}

pub fn raw_to_employee_context(raw: &RawEmployee) -> Result<EmployeeContext, EmpDeductionError> {
    Ok(EmployeeContext {
        identity: EmployeeIdentity {
            emp_id: raw.employee_id.clone(),
            name: Some(raw.full_name.clone()),
        },
        base_salary: raw.base_salary,
        allowances: build_allowances(&raw.allowances),
        deductions: build_deductions(&raw.deductions)?,
        pay_period: raw.pay_period.clone(),
    })
}

pub fn load_employee_contexts(
    path: impl AsRef<Path>,
) -> Result<Vec<(RawEmployee, EmployeeContext)>, FixtureError> {
    let raw_emps = load_employees(path)?;
    raw_emps
        .into_iter()
        .map(|raw| {
            let ctx = raw_to_employee_context(&raw)?;
            Ok((raw, ctx))
        })
        .collect()
}
