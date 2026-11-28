//! The native half of the `swift_payroll_engine` Python package: JSON text in, JSON text out.
//! `swift_payroll_engine/__init__.py` turns Python objects into that text and back.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use swift_payroll_core::{Compiled, Detail, Input, Structure};

fn bad(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

fn detail(explain: bool) -> Detail {
    if explain {
        Detail::Explained
    } else {
        Detail::Lines
    }
}

/// A compiled salary structure.
#[pyclass(frozen)]
struct Payroll {
    compiled: Compiled,
}

#[pymethods]
impl Payroll {
    #[new]
    fn new(structure: &str) -> PyResult<Self> {
        let structure: Structure = serde_json::from_str(structure).map_err(bad)?;
        Ok(Self { compiled: Compiled::new(&structure).map_err(bad)? })
    }

    #[getter]
    fn warnings(&self) -> Vec<String> {
        self.compiled.warnings().iter().map(ToString::to_string).collect()
    }

    #[pyo3(signature = (employee, explain = false))]
    fn run(&self, py: Python<'_>, employee: &str, explain: bool) -> PyResult<String> {
        let input: Input = serde_json::from_str(employee).map_err(bad)?;
        let slip = py.detach(|| self.compiled.run_with(&input, detail(explain))).map_err(bad)?;
        serde_json::to_string(&slip).map_err(bad)
    }

    /// One result per employee, in the CLI's shape: a payslip or `{"employee_id", "error"}`.
    /// Other Python threads keep running while it calculates (the GIL is released).
    #[pyo3(signature = (employees, explain = false))]
    fn run_all(&self, py: Python<'_>, employees: &str, explain: bool) -> PyResult<String> {
        let inputs: Vec<Input> = serde_json::from_str(employees).map_err(bad)?;
        // run_all refuses an empty or repeated employee_id itself, so the same person is never paid twice.
        let output: Vec<serde_json::Value> = py
            .detach(|| self.compiled.run_all(&inputs, detail(explain)))
            .into_iter()
            .zip(&inputs)
            .map(|(result, input)| match result {
                Ok(slip) => serde_json::to_value(slip)
                    .unwrap_or_else(|e| serde_json::json!({ "employee_id": input.employee_id, "error": format!("the payslip could not be written: {e}") })),
                Err(error) => serde_json::json!({ "employee_id": input.employee_id, "error": error.to_string() }),
            })
            .collect();
        serde_json::to_string(&output).map_err(bad)
    }
}

#[pymodule]
fn _native(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<Payroll>()
}
