//! The `swift-payroll-engine` npm package: a salary structure compiled once, then payslips as plain objects.
//! Values cross the boundary as JSON text (`JSON.stringify` / `JSON.parse`), so amounts stay decimal strings.

use swift_payroll_core::{Compiled, Detail, Input, Structure};
use wasm_bindgen::prelude::*;

fn bad(e: impl std::fmt::Display) -> JsError {
    JsError::new(&e.to_string())
}

fn from_js<T: serde::de::DeserializeOwned>(value: &JsValue) -> Result<T, JsError> {
    // A string is taken as JSON text already, so callers can pass big amounts without a JS number in between.
    let text = match value.as_string() {
        Some(text) => text,
        // `JSON.stringify` gives no text at all for `undefined` or a function.
        None => js_sys::JSON::stringify(value).ok().and_then(|text| text.as_string()).ok_or_else(|| bad("expected an object, an array or JSON text"))?,
    };
    serde_json::from_str(&text).map_err(bad)
}

fn to_js(value: &impl serde::Serialize) -> Result<JsValue, JsError> {
    js_sys::JSON::parse(&serde_json::to_string(value).map_err(bad)?).map_err(|_| bad("the result could not be read as JSON"))
}

fn detail(explain: Option<bool>) -> Detail {
    if explain.unwrap_or(false) {
        Detail::Explained
    } else {
        Detail::Lines
    }
}

/// A compiled salary structure. Throws if the structure is invalid.
#[wasm_bindgen]
pub struct Payroll {
    compiled: Compiled,
}

#[wasm_bindgen]
impl Payroll {
    /// Compiles a structure (an object, or its JSON text).
    #[wasm_bindgen(constructor)]
    pub fn new(structure: JsValue) -> Result<Payroll, JsError> {
        let structure: Structure = from_js(&structure)?;
        Ok(Payroll { compiled: Compiled::new(&structure).map_err(bad)? })
    }

    /// Warnings found while compiling.
    #[wasm_bindgen(getter)]
    pub fn warnings(&self) -> Vec<String> {
        self.compiled.warnings().iter().map(ToString::to_string).collect()
    }

    /// One payslip. Throws if this employee can't be calculated.
    pub fn run(&self, employee: JsValue, explain: Option<bool>) -> Result<JsValue, JsError> {
        let input: Input = from_js(&employee)?;
        to_js(&self.compiled.run_with(&input, detail(explain)).map_err(bad)?)
    }

    /// One result per employee, in the CLI's shape: a payslip or `{ employee_id, error }`.
    #[wasm_bindgen(js_name = runAll)]
    pub fn run_all(&self, employees: JsValue, explain: Option<bool>) -> Result<JsValue, JsError> {
        let inputs: Vec<Input> = from_js(&employees)?;
        // run_all refuses an empty or repeated employee_id itself, so the same person is never paid twice.
        let output: Vec<serde_json::Value> = self
            .compiled
            .run_all(&inputs, detail(explain))
            .into_iter()
            .zip(&inputs)
            .map(|(result, input)| match result {
                Ok(slip) => serde_json::to_value(slip)
                    .unwrap_or_else(|e| serde_json::json!({ "employee_id": input.employee_id, "error": format!("the payslip could not be written: {e}") })),
                Err(error) => serde_json::json!({ "employee_id": input.employee_id, "error": error.to_string() }),
            })
            .collect();
        to_js(&output)
    }
}
