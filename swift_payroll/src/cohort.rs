use std::sync::Arc;

use rust_decimal::Decimal;
use rust_decimal::prelude::FromPrimitive;
use sp_engine::models::deduction::EmpDeductionError;
use sp_engine::models::emp_context::{EmployeeContext, EmployeeIdentity};

use crate::cli::models::RawEmployee;
use crate::fixture::raw_to_employee_context;

pub struct ProfileCohort {
    templates: Vec<Arc<EmployeeContext>>,
}

impl ProfileCohort {
    pub fn from_profiles(profiles: &[RawEmployee]) -> Result<Self, EmpDeductionError> {
        let templates: Vec<Arc<EmployeeContext>> = profiles
            .iter()
            .map(raw_to_employee_context)
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .map(Arc::new)
            .collect();
        assert!(!templates.is_empty(), "cohort requires at least one profile");
        Ok(Self { templates })
    }

    pub fn template_count(&self) -> usize {
        self.templates.len()
    }

    pub fn employee_at(&self, index: usize) -> EmployeeContext {
        let base = &self.templates[index % self.templates.len()];
        let mut emp = EmployeeContext {
            identity: EmployeeIdentity {
                emp_id: format!("BENCH-{index:07}"),
                name: Some(format!("Bench Employee {index}")),
            },
            base_salary: base.base_salary,
            allowances: base.allowances.clone(),
            deductions: base.deductions.clone(),
            pay_period: base.pay_period.clone(),
        };
        let bump = Decimal::from_i64(((index as i64) % 97) * 25).unwrap_or(Decimal::ZERO);
        emp.base_salary += bump;
        emp
    }
}

pub fn expand_profiles(
    profiles: &[RawEmployee],
    count: usize,
) -> Result<Vec<EmployeeContext>, EmpDeductionError> {
    let cohort = ProfileCohort::from_profiles(profiles)?;
    Ok((0..count).map(|i| cohort.employee_at(i)).collect())
}
