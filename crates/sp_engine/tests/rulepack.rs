//! Rule packs joined into a structure: the substitution that turns a pack's inputs and constants into
//! names of the structure must be exact. It is text surgery on a formula, so it is tested like a parser.

use std::collections::BTreeMap;

use serde_json::json;
use sp_engine::{Compiled, Input, Kind, RulePack, Structure};

fn pack(inputs: &[&str], constants: serde_json::Value, expression: &str) -> RulePack {
    let description = json!({
        "id": "xx.demo", "country": "XX", "kind": "income_tax", "currency": "XXX",
        "valid_from": "2020-01-01", "source": "test", "inputs": inputs, "constants": constants,
        "tests": []
    });
    RulePack::parse(&description.to_string(), expression).unwrap()
}

fn args(pairs: &[(&str, &str)]) -> BTreeMap<String, String> {
    pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
}

#[test]
fn an_argument_is_not_rewritten_by_the_next_substitution() {
    // x is given "y + 1" and y is given "z": the "y" inside x's argument is the structure's own name `y`,
    // not the pack's input.
    let p = pack(&["x", "y"], json!({}), "x * y");
    let (_, c) = p.as_component("tax", "Tax", Kind::Deduction, &args(&[("x", "y + 1"), ("y", "z")])).unwrap();
    assert_eq!(c.formula.as_deref(), Some("(y + 1) * (z)"));
}

#[test]
fn strings_comments_and_member_keys_are_left_alone() {
    let p = pack(&["rate"], json!({"table": {"rate": 5}}), "// the rate\nrate > 0 ? string(\"rate\") == \"rate\" ? table.rate : 0 : 0");
    let (_, c) = p.as_component("tax", "Tax", Kind::Deduction, &args(&[("rate", "income")])).unwrap();
    let formula = c.formula.unwrap();
    assert!(formula.contains("string(\"rate\") == \"rate\""), "string literals were changed: {formula}");
    assert!(formula.contains("xx_demo_table.rate"), "the member key after `.` must stay: {formula}");
    assert!(formula.contains("// the rate"), "comment changed: {formula}");
    assert!(formula.contains("(income) > 0"), "{formula}");
}

#[test]
fn a_name_that_is_also_a_function_is_only_replaced_where_it_is_a_name() {
    let p = pack(&["min"], json!({}), "min(min, 5)");
    let (_, c) = p.as_component("tax", "Tax", Kind::Deduction, &args(&[("min", "floor_value")])).unwrap();
    assert_eq!(c.formula.as_deref(), Some("min((floor_value), 5)"));
}

#[test]
fn a_pack_in_a_structure_gives_the_same_answer_as_on_its_own() {
    let p = pack(&["income", "periods"], json!({"band": [[1000, 0], [null, 0.1]]}), "bracket(income * periods, band) / periods");
    let (constants, component) = p.as_component("tax", "Tax", Kind::Deduction, &args(&[("income", "base_salary"), ("periods", "12")])).unwrap();
    let structure: Structure = serde_json::from_value(json!({
        "id": "s", "version": "1", "currency": "XXX",
        "inputs": [{"name": "base_salary"}],
        "constants": constants,
        "components": [
            {"id": "base", "label": "Base", "kind": "earning", "input": "base_salary"},
            component
        ]
    })).unwrap();
    let compiled = Compiled::new(&structure).unwrap();
    let input = Input { employee_id: "e".into(), variables: BTreeMap::from([("base_salary".into(), json!(1000))]) };
    let slip = compiled.run(&input, false).unwrap();
    let alone = p.evaluate(&BTreeMap::from([("income".to_string(), json!(1000)), ("periods".to_string(), json!(12))])).unwrap();
    assert_eq!(slip.lines.iter().find(|l| l.id == "tax").unwrap().amount, alone);
}
