use std::collections::BTreeMap;

use rust_decimal::Decimal;

use super::*;

type Test = std::result::Result<(), Box<dyn std::error::Error>>;

fn d(text: &str) -> std::result::Result<Decimal, rust_decimal::Error> {
    Decimal::from_str_exact(text)
}

fn run(source: &str, vars: &[(&str, Value)]) -> Result<Value> {
    let env = MapEnv(vars.iter().map(|(k, v)| (k.to_string(), v.clone())).collect::<BTreeMap<_, _>>());
    compile(source)?.eval(&env)
}

fn num(text: &str) -> std::result::Result<Value, rust_decimal::Error> {
    Ok(Value::Num(d(text)?))
}

#[test]
fn arithmetic_is_exact() -> Test {
    assert_eq!(run("0.1 + 0.2", &[])?, num("0.3")?, "no binary floating point");
    assert_eq!(run("100 * 0.15", &[])?, num("15.00")?);
    assert_eq!(run("7 % 3", &[])?, num("1")?);
    assert_eq!(run("-(2 + 3) * 2", &[])?, num("-10")?);
    assert_eq!(run("10 / 4", &[])?, num("2.5")?);
    assert!(run("1 / 0", &[]).is_err());
    assert!(run("1 % 0", &[]).is_err());
    Ok(())
}

#[test]
fn precedence_and_conditions() -> Test {
    assert_eq!(run("1 + 2 * 3", &[])?, num("7")?);
    assert_eq!(run("(1 + 2) * 3", &[])?, num("9")?);
    assert_eq!(run("1 < 2 && 2 < 3 || false", &[])?, Value::Bool(true));
    assert_eq!(run("x > 10 ? 1 : x > 5 ? 2 : 3", &[("x", Value::from(7))])?, num("2")?);
    assert_eq!(run("!(1 == 2)", &[])?, Value::Bool(true));
    // The branch not taken is never evaluated.
    assert_eq!(run("true ? 1 : 1 / 0", &[])?, num("1")?);
    assert_eq!(run("false && (1 / 0 > 1)", &[])?, Value::Bool(false));
    Ok(())
}

#[test]
fn values_lists_maps_and_members() -> Test {
    assert_eq!(run("[1, 2, 3][1]", &[])?, num("2")?);
    assert_eq!(run("{'a': 5}.a + {'a': 5}['a']", &[])?, num("10")?);
    assert_eq!(run("3 in [1, 2, 3]", &[])?, Value::Bool(true));
    assert_eq!(run("'a' in {'a': 1}", &[])?, Value::Bool(true));
    assert_eq!(run("'ab' + \"c\"", &[])?, Value::Str("abc".into()));
    assert!(run("[1][5]", &[]).is_err());
    assert!(run("[1, 2][1.9]", &[]).is_err(), "fractional indexes must not truncate");
    assert!(run("[1, 2][-0.5]", &[]).is_err(), "negative fractional indexes must not become zero");
    assert!(run("[1, 2][-1]", &[]).is_err(), "negative indexes are invalid");
    assert!(run("x", &[]).is_err(), "an unknown name is an error, never zero");
    assert!(run("3.5.floor", &[]).is_err(), "a number has no members");
    Ok(())
}

#[test]
fn function_call_constraints_check_arity_and_literal_string_arguments() -> Test {
    assert!(compile("total(\"earning\")")?.calls_match_literals("total", 1, &[0]));
    assert!(!compile("total(tag)")?.calls_match_literals("total", 1, &[0]));
    assert!(!compile("total()")?.calls_match_literals("total", 1, &[0]));
    assert!(!compile("total(\"earning\", tag)")?.calls_match_literals("total", 1, &[0]));
    assert!(compile("other(tag)")?.calls_match_literals("total", 1, &[0]));
    Ok(())
}

#[test]
fn comments_and_errors_say_where() -> Test {
    assert_eq!(run("// the base\n1 + 1 // twice", &[])?, num("2")?);
    let error = compile("1 +").err().ok_or("it should not compile")?;
    assert!(error.at.is_some());
    assert!(compile("1 + * 2").is_err());
    assert!(compile("(1").is_err());
    assert!(compile("'open").is_err());
    assert!(compile("1 2").is_err());
    assert!(compile(&"(".repeat(200)).is_err(), "deep nesting is refused");
    Ok(())
}

#[test]
fn rounding_functions() -> Test {
    assert_eq!(run("round(2.5)", &[])?, num("3")?);
    assert_eq!(run("round_even(2.5)", &[])?, num("2")?);
    assert_eq!(run("round(-2.5)", &[])?, num("-3")?);
    assert_eq!(run("round(1.005, 2)", &[])?, num("1.01")?);
    assert_eq!(run("floor(-1.5) + ceil(1.2) + trunc(-1.9)", &[])?, num("-1")?);
    assert_eq!(run("clamp(15, 0, 10)", &[])?, num("10")?);
    assert_eq!(run("min(3, 1, 2) + max([4, 9])", &[])?, num("10")?);
    assert_eq!(run("pct(2500, 5)", &[])?, num("125")?);
    assert_eq!(run("prorate(3000, 10, 30)", &[])?, num("1000")?);
    assert!(run("prorate(1, 1, 0)", &[]).is_err());
    assert_eq!(run("between(5, 5, 10)", &[])?, Value::Bool(true));
    Ok(())
}

#[test]
fn a_bracket_table_is_progressive() -> Test {
    let table = "[[24000, 0], [48000, 0.05], [72000, 0.10], [null, 0.20]]";
    assert_eq!(run(&format!("bracket(0, {table})"), &[])?, num("0")?);
    assert_eq!(run(&format!("bracket(24000, {table})"), &[])?, num("0")?);
    assert_eq!(run(&format!("bracket(30000, {table})"), &[])?, num("300.00")?);
    assert_eq!(run(&format!("bracket(60000, {table})"), &[])?, num("2400.00")?, "24000 at 5% = 1200, 12000 at 10% = 1200");
    assert_eq!(run(&format!("bracket(100000, {table})"), &[])?, num("9200.00")?);
    assert_eq!(run(&format!("rate_at(48000, {table}) + rate_at(48000.01, {table})"), &[])?, num("0.15")?);
    // Without an open top band an amount above the table is an error, not a silent cap.
    assert!(run("bracket(100, [[50, 0.1]])", &[]).is_err());
    assert!(run("bracket(10, [[50, 0.1], [40, 0.2]])", &[]).is_err());
    assert!(run("bracket(10, [[null, 0.1], [40, 0.2]])", &[]).is_err());
    assert!(run("bracket(10, [])", &[]).is_err());
    Ok(())
}

#[test]
fn errors_have_a_kind() -> Test {
    let kind = |source: &str| run(source, &[]).unwrap_err().kind;
    assert_eq!(compile("1 +").unwrap_err().kind, ErrorKind::Syntax);
    assert_eq!(compile(&"(".repeat(200)).unwrap_err().kind, ErrorKind::TooDeep);
    assert_eq!(kind("x"), ErrorKind::Undefined);
    assert_eq!(kind("1 + 'a'"), ErrorKind::Type);
    assert_eq!(kind("1 / 0"), ErrorKind::DivisionByZero);
    assert_eq!(kind("79228162514264337593543950335 * 2"), ErrorKind::Overflow);
    assert_eq!(kind("abs(1, 2)"), ErrorKind::Arity);
    assert_eq!(kind("nope(1)"), ErrorKind::UnknownFunction);
    assert_eq!(kind("bracket(1, [[5, 0.1], [2, 0.2]])"), ErrorKind::InvalidArgument);
    assert_eq!(kind("0.00000000000000123456789 * 0.0000000000000012345"), ErrorKind::Precision, "never a silent zero");
    Ok(())
}

#[test]
fn exact_precision_refuses_what_rounding_would_hide() -> Test {
    let exact = |source: &str| compile(source).and_then(|e| e.eval_with(&MapEnv::default(), Precision::Exact));
    assert_eq!(exact("1 / 4")?, num("0.25")?);
    assert_eq!(exact("pct(1000, 7.5) + prorate(3000, 10, 30)")?, num("1075.0")?);
    assert_eq!(exact("1 / 3").unwrap_err().kind, ErrorKind::Precision);
    assert_eq!(exact("prorate(100, 1, 3)").unwrap_err().kind, ErrorKind::Precision);
    assert_eq!(run("1 / 3 * 3", &[])?, num("0.9999999999999999999999999999")?, "rounding mode rounds at 28 digits");
    Ok(())
}

#[test]
fn a_character_outside_ascii_is_named_whole() {
    let error = compile("1 € 2").unwrap_err();
    assert!(error.message.contains('€'), "{}", error.message);
}

#[test]
fn tables_can_be_found_and_checked_before_running() -> Test {
    let expr = compile("bracket(pay, paye) + rate_at(pay, [[1, 0]]) + bracket(pay * 2, other)")?;
    assert_eq!(expr.identifier_arguments("bracket", 1), ["paye", "other"]);
    let good = run("[[100, 0], [null, 0.1]]", &[])?;
    assert!(builtins::check_bracket_table(&good).is_ok());
    let bad = run("[[null, 0], [100, 0.1]]", &[])?;
    assert_eq!(builtins::check_bracket_table(&bad).unwrap_err().kind, ErrorKind::InvalidArgument);
    Ok(())
}

#[test]
fn a_list_position_too_large_is_not_in_the_list() {
    let env = MapEnv::default();
    let error = compile("[1, 2][5000000000]").unwrap().eval(&env).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Undefined);
    assert!(error.message.contains("there is no item 5000000000"), "{}", error.message);
    assert_eq!(compile("[1, 2][-1]").unwrap().eval(&env).unwrap_err().kind, ErrorKind::InvalidArgument);
}
