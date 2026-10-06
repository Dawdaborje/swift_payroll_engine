use serde_json::json;

use crate::{Compiled, EngineError, Input, Structure};

type Test = Result<(), Box<dyn std::error::Error>>;

// Illustrative figures to exercise the engine; not any country's law.
fn demo() -> Result<Structure, serde_json::Error> {
    serde_json::from_value(json!({
        "id": "demo", "version": "1", "currency": "GMD",
        "inputs": [
            { "name": "base_salary" },
            { "name": "transport", "default": 0 },
            { "name": "periods_per_year", "default": 12 },
            { "name": "loan", "default": 0 }
        ],
        "constants": { "paye_table": [[288000, 0], [576000, 0.10], [null, 0.20]] },
        "components": [
            { "id": "base", "label": "Base salary", "kind": "earning", "input": "base_salary", "tags": ["pensionable"] },
            { "id": "transport_allowance", "label": "Transport", "kind": "earning", "input": "transport" },
            { "id": "housing", "label": "Housing (20% of base)", "kind": "earning", "formula": "pct(base, 20)" },
            { "id": "gross", "label": "Gross", "kind": "info", "formula": "total(\"earning\")" },
            { "id": "social", "label": "Social security (5% of pensionable pay)", "kind": "deduction", "tags": ["pre_tax"], "formula": "pct(total(\"pensionable\"), 5)" },
            { "id": "taxable", "label": "Taxable pay", "kind": "info", "formula": "gross - total(\"pre_tax\")" },
            { "id": "paye", "label": "Income tax", "kind": "deduction",
              "formula": "bracket(taxable * periods_per_year, paye_table) / periods_per_year" },
            { "id": "loan_repayment", "label": "Loan", "kind": "deduction", "input": "loan", "when": "loan > 0" },
            { "id": "employer_social", "label": "Employer social security (10%)", "kind": "employer", "formula": "pct(total(\"pensionable\"), 10)" }
        ]
    }))
}

fn person(variables: serde_json::Value) -> Result<Input, serde_json::Error> {
    serde_json::from_value(json!({ "employee_id": "e1", "variables": variables }))
}

#[test]
fn a_payslip_is_exact() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let slip = compiled.run(&person(json!({ "base_salary": 50000, "transport": 2000 }))?, false)?;
    // 62000 gross; social 2500; taxable 59500; yearly 714000: 28800 + 27600 = 56400; monthly 4700.
    let amount = |id: &str| slip.lines.iter().find(|l| l.id == id).map(|l| l.amount.to_string());
    assert_eq!(slip.gross.to_string(), "62000.00");
    assert_eq!(amount("housing").as_deref(), Some("10000.00"));
    assert_eq!(amount("social").as_deref(), Some("2500.00"));
    assert_eq!(amount("taxable").as_deref(), Some("59500.00"));
    assert_eq!(amount("paye").as_deref(), Some("4700.00"));
    assert_eq!(slip.deductions.to_string(), "7200.00");
    assert_eq!(slip.net.to_string(), "54800.00");
    assert_eq!(slip.employer_cost.to_string(), "5000.00");
    assert_eq!(slip.structure_version, "1");
    Ok(())
}

#[test]
fn lines_keep_the_order_they_were_written_in() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let slip = compiled.run(&person(json!({ "base_salary": 1000 }))?, false)?;
    let ids: Vec<&str> = slip.lines.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["base", "transport_allowance", "housing", "gross", "social", "taxable", "paye", "loan_repayment", "employer_social"]);
    Ok(())
}

#[test]
fn a_condition_can_switch_a_line_off() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let without = compiled.run(&person(json!({ "base_salary": 50000 }))?, false)?;
    let line = without.lines.iter().find(|l| l.id == "loan_repayment").ok_or("no loan line")?;
    assert!(!line.applied && line.amount.to_string() == "0.00");
    let with = compiled.run(&person(json!({ "base_salary": 50000, "loan": 800 }))?, false)?;
    assert_eq!(with.deductions - without.deductions, rust_decimal::Decimal::from(800));
    Ok(())
}

#[test]
fn explain_shows_the_formula_and_what_it_read() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let slip = compiled.run(&person(json!({ "base_salary": 50000 }))?, true)?;
    let paye = slip.lines.iter().find(|l| l.id == "paye").and_then(|l| l.explain.as_ref()).ok_or("no explanation")?;
    assert!(paye.source.starts_with("bracket("));
    assert!(paye.reads.iter().any(|r| r == "taxable") && paye.reads.iter().any(|r| r == "paye_table"));
    Ok(())
}

#[test]
fn the_calculation_order_follows_the_dependencies_not_the_writing() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "base_salary" }],
        "components": [
            { "id": "tax", "label": "Tax", "kind": "deduction", "formula": "pct(taxable, 10)" },
            { "id": "taxable", "label": "Taxable", "kind": "info", "formula": "base" },
            { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" }
        ]
    }))?;
    let compiled = Compiled::new(&structure)?;
    assert_eq!(compiled.calculation_order(), ["base", "taxable", "tax"]);
    let slip = compiled.run(&person(json!({ "base_salary": 1000 }))?, false)?;
    assert_eq!(slip.net.to_string(), "900.00");
    Ok(())
}

fn refused(structure: serde_json::Value) -> Result<String, Box<dyn std::error::Error>> {
    let structure: Structure = serde_json::from_value(structure)?;
    match Compiled::new(&structure) {
        Ok(_) => Err("it should have been refused".into()),
        Err(error) => Ok(error.to_string()),
    }
}

#[test]
fn a_bad_structure_is_refused_before_any_payroll_runs() -> Test {
    let base = |components: serde_json::Value| json!({ "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x" }], "components": components });
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "b + 1" }, { "id": "b", "label": "B", "kind": "earning", "formula": "a + 1" }])))?.contains("wait on each other"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "a + 1" }])))?.contains("refers to itself"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "typo + 1" }])))?.contains("typo"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "total(\"nothing\")" }])))?.contains("nothing"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "tags": ["t"], "formula": "total(\"t\")" }])))?.contains("itself"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning" }])))?.contains("exactly one"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "amount": 1, "input": "x" }])))?.contains("exactly one"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "input": "nope" }])))?.contains("nope"));
    assert!(refused(base(json!([{ "id": "A b", "label": "A", "kind": "earning", "amount": 1 }])))?.contains("not a name"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "amount": 1 }, { "id": "a", "label": "A", "kind": "earning", "amount": 1 }])))?.contains("twice"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "1 +" }])))?.contains("formula"));
    assert!(refused(base(json!([])))?.contains("no components"));
    Ok(())
}

#[test]
fn a_person_with_wrong_input_gets_a_clear_error() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    assert!(matches!(compiled.run(&person(json!({}))?, false), Err(EngineError::Input { .. })), "base_salary is required");
    assert!(matches!(compiled.run(&person(json!({ "base_salary": 1, "bonus": 5 }))?, false), Err(EngineError::Input { .. })), "an unknown input is a typo, not ignored");
    assert!(matches!(compiled.run(&person(json!({ "base_salary": "lots" }))?, false), Err(EngineError::Component { .. })));
    Ok(())
}

#[test]
fn net_pay_below_zero_follows_the_structures_policy() -> Test {
    let make = |policy: &str| -> Result<Compiled, Box<dyn std::error::Error>> {
        let structure: Structure = serde_json::from_value(json!({
            "id": "s", "version": "1", "currency": "GMD", "negative_net": policy, "inputs": [{ "name": "base_salary" }, { "name": "loan_amount" }],
            "components": [
                { "id": "base", "label": "Base", "kind": "earning", "input": "base_salary" },
                { "id": "loan", "label": "Loan", "kind": "deduction", "input": "loan_amount" }
            ]
        }))?;
        Ok(Compiled::new(&structure)?)
    };
    let input = person(json!({ "base_salary": 100, "loan_amount": 130 }))?;
    assert!(matches!(make("error")?.run(&input, false), Err(EngineError::NegativeNet { .. })));
    let held = make("zero")?.run(&input, false)?;
    assert_eq!((held.net.to_string(), held.shortfall.to_string()), ("0.00".to_string(), "30.00".to_string()));
    assert_eq!(make("allow")?.run(&input, false)?.net.to_string(), "-30.00");
    Ok(())
}

#[test]
fn a_negative_line_is_an_error_unless_allowed() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x" }],
        "components": [{ "id": "a", "label": "A", "kind": "earning", "formula": "x - 10" }]
    }))?;
    let compiled = Compiled::new(&structure)?;
    assert!(compiled.run(&person(json!({ "x": 5 }))?, false).is_err());
    assert!(compiled.run(&person(json!({ "x": 15 }))?, false).is_ok());
    Ok(())
}

#[test]
fn rounding_is_per_line_and_totals_add_the_rounded_lines() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x" }],
        "components": [
            { "id": "a", "label": "A", "kind": "earning", "formula": "x / 3" },
            { "id": "b", "label": "B", "kind": "earning", "formula": "x / 3" },
            { "id": "c", "label": "C", "kind": "earning", "formula": "x / 3" },
            { "id": "d", "label": "D", "kind": "deduction", "formula": "2.675", "round": { "digits": 2, "mode": "half_even" } }
        ]
    }))?;
    let compiled = Compiled::new(&structure)?;
    let slip = compiled.run(&person(json!({ "x": 100 }))?, false)?;
    // 33.33 three times: the cent lost to rounding is not invented back.
    assert_eq!(slip.gross.to_string(), "99.99");
    assert_eq!(slip.deductions.to_string(), "2.68");
    assert_eq!(slip.net.to_string(), "97.31");
    Ok(())
}

#[test]
fn many_people_are_independent() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let inputs = vec![person(json!({ "base_salary": 50000 }))?, person(json!({}))?, person(json!({ "base_salary": 30000 }))?];
    let results = compiled.run_all(&inputs, false);
    assert!(results[0].is_ok() && results[1].is_err() && results[2].is_ok());
    Ok(())
}

// ---- rule packs

use std::collections::BTreeMap;

use crate::{Kind, RulePack};

const PACK: &str = r#"{
  "id": "xx.paye.demo", "country": "xx", "kind": "income_tax", "currency": "GMD", "valid_from": "2025-01-01",
  "source": "illustrative only", "verified": false,
  "inputs": ["taxable", "periods_per_year"],
  "constants": { "brackets": [[288000, 0], [576000, 0.10], [null, 0.20]] },
  "tests": [
    { "name": "monthly 59500", "inputs": { "taxable": 59500, "periods_per_year": 12 }, "expect": "4700.00" },
    { "name": "below the free band", "inputs": { "taxable": 20000, "periods_per_year": 12 }, "expect": "0.00" }
  ]
}"#;

const EXPRESSION: &str = "// tax for one period\nbracket(taxable * periods_per_year, brackets) / periods_per_year";

#[test]
fn a_rule_pack_runs_its_own_tests() -> Test {
    let pack = RulePack::parse(PACK, EXPRESSION)?;
    let results = pack.run_tests();
    assert_eq!(results.len(), 2);
    assert!(results.iter().all(|(_, r)| r.is_ok()), "{results:?}");
    assert!(pack.in_force_on("2025-06-01") && !pack.in_force_on("2024-12-31"));
    Ok(())
}

#[test]
fn a_wrong_expected_value_fails_the_pack() -> Test {
    let wrong = PACK.replace("\"4700.00\"", "\"4701.00\"");
    let pack = RulePack::parse(&wrong, EXPRESSION)?;
    assert!(pack.run_tests().iter().any(|(_, r)| r.is_err()));
    Ok(())
}

#[test]
fn a_rule_pack_is_refused_when_it_cannot_work() -> Test {
    assert!(RulePack::parse(PACK, "// nothing here\n").is_err(), "an empty expression");
    assert!(RulePack::parse(PACK, "taxable * rate").is_err(), "an unknown name");
    assert!(RulePack::parse(PACK, "taxable +").is_err(), "a syntax error");
    assert!(RulePack::parse("{ }", EXPRESSION).is_err());
    let pack = RulePack::parse(PACK, EXPRESSION)?;
    let mut inputs: BTreeMap<String, serde_json::Value> = BTreeMap::new();
    inputs.insert("taxable".into(), json!(1));
    assert!(pack.evaluate(&inputs).is_err(), "a missing input");
    inputs.insert("periods_per_year".into(), json!(12));
    inputs.insert("bonus".into(), json!(1));
    assert!(pack.evaluate(&inputs).is_err(), "an unknown input");
    Ok(())
}

#[test]
fn a_pack_becomes_a_component_of_a_structure() -> Test {
    let pack = RulePack::parse(PACK, EXPRESSION)?;
    let arguments = BTreeMap::from([("taxable".to_string(), "taxable".to_string()), ("periods_per_year".to_string(), "periods_per_year".to_string())]);
    let (constants, paye) = pack.as_component("paye", "Income tax", Kind::Deduction, &arguments)?;
    let mut structure = demo()?;
    structure.constants.extend(constants);
    structure.components.retain(|c| c.id != "paye");
    structure.components.push(paye);
    let compiled = Compiled::new(&structure)?;
    let slip = compiled.run(&person(json!({ "base_salary": 50000, "transport": 2000 }))?, false)?;
    assert_eq!(slip.lines.iter().find(|l| l.id == "paye").map(|l| l.amount.to_string()).as_deref(), Some("4700.00"), "the same answer as the formula written by hand");
    Ok(())
}
