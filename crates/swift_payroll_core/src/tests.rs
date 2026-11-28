use serde_json::json;

use crate::{Compiled, Detail, EngineError, Input, Structure};

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
    let slip = compiled.run(&person(json!({ "base_salary": 50000, "transport": 2000 }))?)?;
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
    let slip = compiled.run(&person(json!({ "base_salary": 1000 }))?)?;
    let ids: Vec<&str> = slip.lines.iter().map(|l| l.id.as_str()).collect();
    assert_eq!(ids, ["base", "transport_allowance", "housing", "gross", "social", "taxable", "paye", "loan_repayment", "employer_social"]);
    Ok(())
}

#[test]
fn a_condition_can_switch_a_line_off() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let without = compiled.run(&person(json!({ "base_salary": 50000 }))?)?;
    let line = without.lines.iter().find(|l| l.id == "loan_repayment").ok_or("no loan line")?;
    assert!(!line.applied && line.amount.to_string() == "0.00");
    let with = compiled.run(&person(json!({ "base_salary": 50000, "loan": 800 }))?)?;
    assert_eq!(with.deductions - without.deductions, rust_decimal::Decimal::from(800));
    Ok(())
}

#[test]
fn explain_shows_the_formula_and_what_it_read() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let slip = compiled.run_explained(&person(json!({ "base_salary": 50000 }))?)?;
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
    let slip = compiled.run(&person(json!({ "base_salary": 1000 }))?)?;
    assert_eq!(slip.net.to_string(), "900.00");
    Ok(())
}

#[test]
fn component_rounding_cannot_exceed_the_supported_precision() -> Test {
    let structure = serde_json::from_value::<Structure>(json!({
        "id": "s", "version": "1", "currency": "GMD", "components": [
            { "id": "base", "label": "Base", "kind": "earning", "amount": 1, "round": { "digits": 9 } }
        ]
    }));
    assert!(structure.unwrap_err().to_string().contains("at most 8 digits"));
    assert!(crate::Rounding::new(9, crate::Mode::HalfUp).is_none());
    Ok(())
}

#[test]
fn total_requires_a_literal_tag_and_orders_all_tag_members_first() -> Test {
    let invalid: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD",
        "inputs": [{ "name": "salary" }, { "name": "group", "default": "earning" }],
        "components": [
            { "id": "deduction", "label": "Dynamic total", "kind": "deduction", "formula": "total(group)" },
            { "id": "base", "label": "Base", "kind": "earning", "input": "salary" }
        ]
    }))?;
    let error = match Compiled::new(&invalid) {
        Ok(_) => return Err("a non-literal total tag must be rejected".into()),
        Err(error) => error,
    };
    assert!(error.to_string().contains("literal tag string"));

    let valid: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD",
        "inputs": [{ "name": "salary" }],
        "components": [
            { "id": "deduction", "label": "All earnings", "kind": "deduction", "formula": "total(\"earning\")" },
            { "id": "base", "label": "Base", "kind": "earning", "input": "salary" }
        ]
    }))?;
    let compiled = Compiled::new(&valid)?;
    assert_eq!(compiled.calculation_order(), ["base", "deduction"]);
    let slip = compiled.run(&person(json!({ "salary": 100 }))?)?;
    assert_eq!(slip.lines[0].amount.to_string(), "100.00");
    assert_eq!(slip.lines[1].amount.to_string(), "100.00");
    assert_eq!(slip.net.to_string(), "0.00");
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
    assert!(refused(base(
        json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "b + 1" }, { "id": "b", "label": "B", "kind": "earning", "formula": "a + 1" }])
    ))?
    .contains("wait on each other"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "a + 1" }])))?.contains("refers to itself"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "typo + 1" }])))?.contains("typo"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "total(\"nothing\")" }])))?.contains("nothing"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "tags": ["t"], "formula": "total(\"t\")" }])))?.contains("itself"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning" }])))?.contains("exactly one"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "amount": 1, "input": "x" }])))?.contains("exactly one"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "input": "nope" }])))?.contains("nope"));
    assert!(refused(base(json!([{ "id": "A b", "label": "A", "kind": "earning", "amount": 1 }])))?.contains("not a name"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "amount": 1 }, { "id": "a", "label": "A", "kind": "earning", "amount": 1 }])))?
        .contains("twice"));
    assert!(refused(base(json!([{ "id": "a", "label": "A", "kind": "earning", "formula": "1 +" }])))?.contains("formula"));
    assert!(refused(base(json!([])))?.contains("no components"));
    Ok(())
}

#[test]
fn a_person_with_wrong_input_gets_a_clear_error() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    assert!(matches!(compiled.run(&person(json!({}))?), Err(EngineError::Input { .. })), "base_salary is required");
    assert!(
        matches!(compiled.run(&person(json!({ "base_salary": 1, "bonus": 5 }))?), Err(EngineError::Input { .. })),
        "an unknown input is a typo, not ignored"
    );
    assert!(matches!(compiled.run(&person(json!({ "base_salary": "lots" }))?), Err(EngineError::Component { .. })));
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
    assert!(matches!(make("error")?.run(&input), Err(EngineError::NegativeNet { .. })));
    let held = make("zero")?.run(&input)?;
    assert_eq!((held.net.to_string(), held.shortfall.to_string()), ("0.00".to_string(), "30.00".to_string()));
    assert_eq!(make("allow")?.run(&input)?.net.to_string(), "-30.00");
    Ok(())
}

#[test]
fn a_negative_line_is_an_error_unless_allowed() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x" }],
        "components": [{ "id": "a", "label": "A", "kind": "earning", "formula": "x - 10" }]
    }))?;
    let compiled = Compiled::new(&structure)?;
    assert!(compiled.run(&person(json!({ "x": 5 }))?).is_err());
    assert!(compiled.run(&person(json!({ "x": 15 }))?).is_ok());
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
    let slip = compiled.run(&person(json!({ "x": 100 }))?)?;
    // 33.33 three times: the cent lost to rounding is not invented back.
    assert_eq!(slip.gross.to_string(), "99.99");
    assert_eq!(slip.deductions.to_string(), "2.68");
    assert_eq!(slip.net.to_string(), "97.31");
    Ok(())
}

#[test]
fn many_people_are_independent() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let named = |id: &str, variables| serde_json::from_value::<Input>(json!({ "employee_id": id, "variables": variables }));
    let inputs = vec![named("e1", json!({ "base_salary": 50000 }))?, named("e2", json!({}))?, named("e3", json!({ "base_salary": 30000 }))?];
    let results = compiled.run_all(&inputs, Detail::Lines);
    assert!(results[0].is_ok() && results[1].is_err() && results[2].is_ok());
    Ok(())
}

#[test]
fn a_run_refuses_empty_and_repeated_ids() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let named = |id: &str| serde_json::from_value::<Input>(json!({ "employee_id": id, "variables": { "base_salary": 1000 } }));
    // The first e1 fails; the second is still refused, so a fixed-up copy is never paid alongside it.
    let failing: Input = serde_json::from_value(json!({ "employee_id": "e1", "variables": {} }))?;
    let inputs = vec![failing, named("e1")?, named("")?, named("e2")?];
    let errors: Vec<String> = compiled.run_all(&inputs, Detail::Lines).into_iter().map(|r| r.err().map(|e| e.to_string()).unwrap_or_default()).collect();
    assert_eq!(errors[1], "e1: this employee_id appears more than once in the run");
    assert_eq!(errors[2], "employee_id is empty");
    assert_eq!(errors[3], "");
    let mut streamed = Vec::new();
    compiled.for_each(&inputs, Detail::Lines, 1, |_, r| streamed.push(r.err().map(|e| e.to_string()).unwrap_or_default()));
    assert_eq!(streamed, errors);
    Ok(())
}

// ---- rule packs

use std::collections::BTreeMap;

use crate::{Kind, PackChecks, RulePack};

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
    assert!(!pack.in_force_on("2025-02-30"), "an invalid date cannot match a validity range");
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
    let (constants, paye) = pack.as_component("paye", "Income tax", Kind::Deduction, &arguments, &PackChecks::new().allow_unverified().any_date())?;
    let mut structure = demo()?;
    structure.constants.extend(constants);
    structure.components.retain(|c| c.id != "paye");
    structure.components.push(paye);
    let compiled = Compiled::new(&structure)?;
    let slip = compiled.run(&person(json!({ "base_salary": 50000, "transport": 2000 }))?)?;
    assert_eq!(
        slip.lines.iter().find(|l| l.id == "paye").map(|l| l.amount.to_string()).as_deref(),
        Some("4700.00"),
        "the same answer as the formula written by hand"
    );
    Ok(())
}

// ---- strictness, priorities, precision, types (the 0.3 hardening)

use crate::{Component, ComponentErrorKind, InputSpec, InputType, NegativeNet, PackChecks as Checks, Payslip, Precision, RuleErrorKind, Warning};

#[test]
fn a_misspelled_field_is_refused_not_ignored() {
    let typo =
        |component: serde_json::Value| serde_json::from_value::<Structure>(json!({ "id": "s", "version": "1", "currency": "GMD", "components": [component] }));
    // `wehn` for `when` used to be dropped, so the dues were taken from everybody.
    let error = typo(json!({ "id": "union", "label": "Union", "kind": "deduction", "amount": 100, "wehn": "is_member" })).unwrap_err();
    assert!(error.to_string().contains("wehn"), "{error}");
    assert!(typo(json!({ "id": "a", "label": "A", "kind": "earning", "amount": 1, "allow_negativ": true })).is_err());
    assert!(typo(json!({ "id": "a", "label": "A", "kind": "earning", "amount": 1, "round": { "digits": 2, "mod": "up" } })).is_err());
    assert!(serde_json::from_value::<Structure>(json!({ "id": "s", "version": "1", "currency": "GMD", "negativ_net": "allow", "components": [] })).is_err());
    assert!(serde_json::from_value::<Input>(json!({ "employee_id": "e", "variabels": {} })).is_err());
    assert!(serde_json::from_value::<Structure>(
        json!({ "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x", "defualt": 0 }], "components": [] })
    )
    .is_err());
}

#[test]
fn inputs_and_constants_nothing_reads_are_reported() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD",
        "inputs": [{ "name": "base" }, { "name": "is_member", "default": false }],
        "constants": { "unused_rate": 0.1 },
        "components": [{ "id": "pay", "label": "Pay", "kind": "earning", "input": "base" }]
    }))?;
    let compiled = Compiled::new(&structure)?;
    assert_eq!(compiled.warnings(), [Warning::UnusedInput("is_member".into()), Warning::UnusedConstant("unused_rate".into())]);
    Ok(())
}

#[test]
fn rounding_to_places_the_currency_does_not_have_is_reported() -> Test {
    let structure = |currency: &str| -> Result<Structure, serde_json::Error> {
        serde_json::from_value(json!({
            "id": "s", "version": "1", "currency": currency, "inputs": [{ "name": "base" }],
            "components": [
                { "id": "pay", "label": "Pay", "kind": "earning", "input": "base" },
                { "id": "rate", "label": "Hourly rate", "kind": "info", "formula": "pay / 160", "round": { "digits": 4 } },
                { "id": "fee", "label": "Fee", "kind": "deduction", "amount": 1, "round": { "digits": 0 } }
            ]
        }))
    };
    let jpy = Compiled::new(&structure("JPY")?)?;
    assert_eq!(jpy.warnings(), [Warning::CurrencyDigits { currency: "JPY".into(), component: None, digits: 2, minor_units: 0 }]);
    let kwd = Compiled::new(&structure("kwd")?)?;
    assert_eq!(kwd.warnings().len(), 2, "the structure's 2 places and `fee`'s 0, not the working figure: {:?}", kwd.warnings());
    assert!(Compiled::new(&structure("USD")?)?.warnings().is_empty());
    Ok(())
}

#[test]
fn reading_a_deduction_that_may_be_cut_is_refused() -> Test {
    // Under `zero`, deductions are cut after the formulas run: taxable pay less a pension that was never
    // taken would under-withhold tax, so a structure that reads a deduction is refused before any payroll.
    let mut structure = demo()?;
    structure.negative_net = crate::NegativeNet::Zero;
    let error = Compiled::new(&structure).unwrap_err().to_string();
    assert!(error.contains("taxable") && error.contains("total(\"pre_tax\")"), "{error}");

    structure.components.retain(|c| !["taxable", "paye"].contains(&c.id.as_str()));
    structure.components.push(crate::Component::formula("match", "Employer match", crate::Kind::Employer, "social"));
    let error = Compiled::new(&structure).unwrap_err().to_string();
    assert!(error.contains("`match` reads `social`"), "{error}");

    // Reading earnings and inputs is fine, and the same structure under `error` is accepted.
    structure.components.pop();
    assert!(Compiled::new(&structure).is_ok());
    assert!(Compiled::new(&demo()?).is_ok());
    Ok(())
}

fn cut(policy: &str) -> Result<Compiled, Box<dyn std::error::Error>> {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "negative_net": policy, "inputs": [{ "name": "base" }],
        "components": [
            { "id": "pay", "label": "Pay", "kind": "earning", "input": "base" },
            { "id": "loan", "label": "Loan", "kind": "deduction", "amount": 800, "priority": 20 },
            { "id": "tax", "label": "Tax", "kind": "deduction", "amount": 500, "priority": 1 },
            { "id": "gym", "label": "Gym", "kind": "deduction", "amount": 50 }
        ]
    }))?;
    Ok(Compiled::new(&structure)?)
}

#[test]
fn holding_net_at_zero_cuts_deductions_by_priority_and_still_adds_up() -> Test {
    let slip = cut("zero")?.run(&person(json!({ "base": 1000 }))?)?;
    let line = |id: &str| slip.lines.iter().find(|l| l.id == id).map(|l| (l.amount.to_string(), l.shortfall.to_string()));
    // Tax (priority 1) is taken whole, the loan (20) gets what is left, the gym (no priority) nothing.
    assert_eq!(line("tax"), Some(("500.00".into(), "0.00".into())));
    assert_eq!(line("loan"), Some(("800.00".into(), "300.00".into())));
    assert_eq!(line("gym"), Some(("50.00".into(), "50.00".into())));
    assert_eq!(
        (slip.gross.to_string(), slip.deductions.to_string(), slip.net.to_string(), slip.shortfall.to_string()),
        ("1000.00".into(), "1000.00".into(), "0.00".into(), "350.00".into())
    );
    assert_eq!(slip.gross - slip.deductions, slip.net, "the payslip adds up");
    let amounts = cut("zero")?.calculate_amounts(&person(json!({ "base": 1000 }))?)?;
    assert_eq!(amounts.shortfalls.iter().copied().sum::<rust_decimal::Decimal>(), slip.shortfall);
    // The shortfall of a line is only on the payslip when there is one.
    let text = serde_json::to_string(&slip)?;
    assert_eq!(text.matches("\"shortfall\"").count(), 3, "two cut lines and the total: {text}");
    Ok(())
}

#[test]
fn the_structure_can_demand_exact_arithmetic() -> Test {
    let make = |precision: &str| -> Result<Compiled, Box<dyn std::error::Error>> {
        let structure: Structure = serde_json::from_value(json!({
            "id": "s", "version": "1", "currency": "GMD", "precision": precision, "inputs": [{ "name": "x" }],
            "components": [{ "id": "third", "label": "A third", "kind": "earning", "formula": "x / 3" }]
        }))?;
        Ok(Compiled::new(&structure)?)
    };
    assert_eq!(make("round")?.run(&person(json!({ "x": 100 }))?)?.gross.to_string(), "33.33");
    match make("exact")?.run(&person(json!({ "x": 100 }))?) {
        Err(EngineError::Component { kind: ComponentErrorKind::Expression(swift_payroll_expr::ErrorKind::Precision), .. }) => {}
        other => return Err(format!("1/3 must not be exact: {other:?}").into()),
    }
    assert_eq!(make("exact")?.run(&person(json!({ "x": 99 }))?)?.gross.to_string(), "33.00");
    assert!(serde_json::from_value::<Structure>(json!({ "id": "s", "version": "1", "currency": "GMD", "precision": "sloppy", "components": [] })).is_err());
    Ok(())
}

#[test]
fn typed_inputs_are_checked_and_decimal_text_is_a_number() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD",
        "inputs": [{ "name": "base", "type": "number" }, { "name": "region", "type": "text", "default": "north" }, { "name": "bonus" }],
        "components": [
            { "id": "pay", "label": "Pay", "kind": "earning", "input": "base" },
            { "id": "extra", "label": "Bonus", "kind": "earning", "input": "bonus" },
            { "id": "north", "label": "North allowance", "kind": "earning", "amount": "12.50", "when": "region == 'north'" },
            { "id": "tax", "label": "Tax", "kind": "deduction", "formula": "pct(base, 10)" }
        ]
    }))?;
    let compiled = Compiled::new(&structure)?;
    // A number of more than 15 digits, given as text, is read exactly even without `exact-json`.
    let slip = compiled.run(&person(json!({ "base": "123456789012345.67", "bonus": "0.10" }))?)?;
    assert_eq!(slip.gross.to_string(), "123456789012358.27");
    assert_eq!(slip.deductions.to_string(), "12345678901234.57");
    match compiled.run(&person(json!({ "base": "1,000", "bonus": 0 }))?) {
        Err(EngineError::Input { input: Some(name), .. }) if name == "base" => {}
        other => return Err(format!("a thousands separator is not a number: {other:?}").into()),
    }
    assert!(matches!(compiled.run(&person(json!({ "base": 1, "bonus": 0, "region": 5 }))?), Err(EngineError::Input { .. })));
    assert!(matches!(
        compiled.run(&person(json!({ "base": 1, "bonus": "lots" }))?),
        Err(EngineError::Component { kind: ComponentErrorKind::InputNotANumber, .. })
    ));
    Ok(())
}

#[test]
fn a_broken_bracket_table_is_refused_before_any_payroll_runs() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "inputs": [{ "name": "x" }],
        "constants": { "bands": [[1000, 0], [500, 0.1], [null, 0.2]] },
        "components": [{ "id": "tax", "label": "Tax", "kind": "deduction", "formula": "x > 10 ? bracket(x, bands) : 0" }]
    }))?;
    match Compiled::new(&structure) {
        Err(EngineError::Structure { component: Some(c), message }) if c == "tax" && message.contains("band limits go up") => Ok(()),
        other => Err(format!("{other:?}").into()),
    }
}

#[test]
fn structures_can_be_built_in_code_and_payslips_read_back() -> Test {
    let mut structure = Structure::new(
        "s",
        "1",
        "GMD",
        vec![
            Component::input("pay", "Pay", Kind::Earning, "base"),
            Component::formula("tax", "Tax", Kind::Deduction, "pct(pay, 10)").priority(1),
            Component::fixed("dues", "Dues", Kind::Deduction, rust_decimal::Decimal::new(2550, 2)).when("member").tag("voluntary"),
        ],
    );
    structure.inputs = vec![InputSpec::required("base").typed(InputType::Number), InputSpec::optional("member", json!(false))];
    structure.negative_net = NegativeNet::Zero;
    structure.precision = Precision::Exact;
    let compiled = Compiled::new(&structure)?;
    let slip = compiled.run_explained(&person(json!({ "base": 1000, "member": true }))?)?;
    assert_eq!(slip.net.to_string(), "874.50");
    let back: Payslip = serde_json::from_str(&serde_json::to_string(&slip)?)?;
    assert_eq!(back, slip, "a stored payslip reads back the same");
    let again: Structure = serde_json::from_str(&serde_json::to_string(&structure)?)?;
    assert_eq!(again, structure, "a structure written out reads back the same");
    Ok(())
}

#[test]
fn errors_say_what_failed_in_a_way_code_can_match() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let error = compiled.run(&person(json!({}))?).unwrap_err();
    assert_eq!(error.employee(), Some("e1"));
    assert!(matches!(&error, EngineError::Input { input: Some(i), .. } if i == "base_salary"));
    assert_eq!(error.to_string(), "e1: `base_salary` is missing");
    let negative: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "GMD", "components": [{ "id": "a", "label": "A", "kind": "earning", "formula": "0 - 1" }]
    }))?;
    assert!(matches!(Compiled::new(&negative)?.run(&person(json!({}))?), Err(EngineError::Component { kind: ComponentErrorKind::Negative, .. })));
    Ok(())
}

#[test]
fn a_compiled_structure_can_be_shared_between_threads() {
    fn shared<T: Send + Sync>() {}
    shared::<Compiled>();
    shared::<EngineError>();
}

#[test]
fn pack_use_is_checked_for_verification_currency_and_date() -> Test {
    let pack = RulePack::parse(PACK, EXPRESSION)?;
    let arguments = BTreeMap::from([("taxable".to_string(), "taxable".to_string()), ("periods_per_year".to_string(), "periods_per_year".to_string())]);
    let refuse = |checks: Checks| pack.as_component("paye", "Income tax", Kind::Deduction, &arguments, &checks).map(|_| ()).unwrap_err().kind;
    assert_eq!(refuse(Checks::new()), RuleErrorKind::Unverified);
    assert_eq!(refuse(Checks::new().allow_unverified().currency("KES")), RuleErrorKind::Currency);
    assert_eq!(refuse(Checks::new().allow_unverified().on("2024-12-31")), RuleErrorKind::NotInForce);
    assert_eq!(refuse(Checks::new().allow_unverified().on("2025-13-01")), RuleErrorKind::NotInForce);
    let checks = Checks::for_structure(&demo()?).allow_unverified().on("2025-06-30");
    assert!(pack.as_component("paye", "Income tax", Kind::Deduction, &arguments, &checks).is_ok());
    let verified = RulePack::parse(&PACK.replace("\"verified\": false", "\"verified\": true"), EXPRESSION)?;
    // Without a pay date even a verified pack is refused: a table out of force must never run unseen.
    assert_eq!(
        verified.as_component("paye", "Income tax", Kind::Deduction, &arguments, &Checks::new()).map(|_| ()).unwrap_err().kind,
        RuleErrorKind::NotInForce
    );
    assert!(verified.as_component("paye", "Income tax", Kind::Deduction, &arguments, &Checks::new().on("2025-06-30")).is_ok());
    let bad_id = RulePack::parse(&PACK.replace("xx.paye.demo", "XX Paye"), EXPRESSION).unwrap_err();
    assert_eq!(bad_id.kind, RuleErrorKind::Description);
    Ok(())
}

#[test]
fn zero_is_never_negative_on_a_payslip() {
    let rounding = crate::Rounding::default();
    assert_eq!(rounding.apply(rust_decimal::Decimal::ZERO - rust_decimal::Decimal::ZERO).to_string(), "0.00");
    assert_eq!(rounding.apply(rust_decimal::Decimal::new(-1, 3)).to_string(), "0.00");
}

#[test]
fn totals_add_up_when_lines_keep_more_digits() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "r", "version": "1", "currency": "GMD",
        "components": [
            { "id": "a", "label": "A", "kind": "earning", "amount": "100.006", "round": { "digits": 4 } },
            { "id": "b", "label": "B", "kind": "deduction", "amount": "0.004", "round": { "digits": 4 } }
        ]
    }))?;
    let slip = Compiled::new(&structure)?.run(&person(json!({}))?)?;
    assert_eq!((slip.gross.to_string(), slip.deductions.to_string(), slip.net.to_string()), ("100.01".into(), "0.00".into(), "100.01".into()));
    Ok(())
}

#[test]
fn exact_precision_refuses_a_rounded_total() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "x", "version": "1", "currency": "GMD", "precision": "exact",
        "components": [
            { "id": "a", "label": "A", "kind": "earning", "amount": "792281625142643375935439503.35" },
            { "id": "b", "label": "B", "kind": "earning", "amount": "0.01" }
        ]
    }))?;
    let result = Compiled::new(&structure)?.run(&person(json!({}))?);
    assert!(matches!(result, Err(EngineError::TooLarge { .. })), "{result:?}");
    Ok(())
}

#[test]
fn an_amount_too_large_for_its_digits_is_an_error() -> Test {
    let structure: Structure = serde_json::from_value(json!({
        "id": "x", "version": "1", "currency": "GMD",
        "components": [{ "id": "a", "label": "A", "kind": "earning", "amount": "7922816251426433759354395033.5" }]
    }))?;
    let result = Compiled::new(&structure)?.run(&person(json!({}))?);
    assert!(matches!(result, Err(EngineError::Component { kind: ComponentErrorKind::TooLarge, .. })), "{result:?}");
    Ok(())
}

#[test]
fn ids_differing_only_by_spaces_are_the_same_person() -> Test {
    let compiled = Compiled::new(&demo()?)?;
    let a: Input = serde_json::from_value(json!({ "employee_id": "e1", "variables": { "base_salary": 1000 } }))?;
    let b: Input = serde_json::from_value(json!({ "employee_id": " e1 ", "variables": { "base_salary": 1000 } }))?;
    let results = compiled.run_all(&[a, b], Detail::Lines);
    assert!(results[0].is_ok());
    assert!(matches!(&results[1], Err(EngineError::Input { message, .. }) if message.contains("more than once")));
    Ok(())
}

#[test]
fn an_explanation_names_the_tags_it_totals() -> Test {
    let slip = Compiled::new(&demo()?)?.run_explained(&person(json!({ "base_salary": 1000 }))?)?;
    let social = slip.lines.iter().find(|l| l.id == "social").ok_or("no social line")?;
    assert_eq!(social.explain.as_ref().ok_or("no explanation")?.reads, vec!["total(\"pensionable\")".to_string()]);
    Ok(())
}
