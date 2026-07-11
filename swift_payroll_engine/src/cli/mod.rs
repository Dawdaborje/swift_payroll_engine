use crate::cli::args::CliArgs;
use crate::fixture::{load_employees, raw_to_employee_context};

pub mod args;
pub mod models;

pub fn run(args: &CliArgs) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(path) = &args.input {
        let raw_emps = load_employees(path)?;
        let contexts = raw_emps
            .iter()
            .map(raw_to_employee_context)
            .collect::<Result<Vec<_>, _>>()?;
        let calc = sp_engine::calculator::CalculationContext {
            emp_contexts: contexts,
        };
        let results = calc.calculate()?;
        for (i, r) in results.iter().enumerate() {
            println!(
                "Employee {}: id = {}, gross = {}, net = {}",
                i + 1,
                r.employee_id,
                r.gross_salary,
                r.net_salary
            );
            for d in &r.deductions {
                println!("  deduction {} = {}", d.name, d.amount);
            }
        }
        Ok(())
    } else {
        println!("No input file supplied. Args: {:?}", args);
        Ok(())
    }
}
