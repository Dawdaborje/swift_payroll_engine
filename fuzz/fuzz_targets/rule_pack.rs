//! Any description and expression read as a rule pack: parsing, running its tests and turning it into a
//! component never panic. The input is the description, a NUL byte, then the expression.
#![no_main]

use std::collections::BTreeMap;

use libfuzzer_sys::fuzz_target;
use swift_payroll_core::{Kind, PackChecks, RulePack};

fuzz_target!(|data: &[u8]| {
    let Ok(text) = std::str::from_utf8(data) else { return };
    let (description, expression) = text.split_once('\0').unwrap_or((text, "x"));
    let Ok(pack) = RulePack::parse(description, expression) else { return };
    let _ = pack.run_tests();
    let _ = pack.in_force_on("2025-06-30");
    let arguments: BTreeMap<String, String> = pack.info.inputs.iter().map(|name| (name.clone(), "1000".to_string())).collect();
    let _ = pack.as_component("rule", "Rule", Kind::Deduction, &arguments, &PackChecks::new().allow_unverified());
});
