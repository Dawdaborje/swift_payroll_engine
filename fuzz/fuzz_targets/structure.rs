//! Any bytes read as a run (a structure and people): reading, compiling and calculating never panic, and a
//! payslip that comes out keeps the engine's invariants.
#![no_main]

use libfuzzer_sys::fuzz_target;
use swift_payroll_core::{Compiled, Decimal, Input, Kind, Structure};

#[derive(serde::Deserialize)]
struct Run {
    structure: Structure,
    #[serde(default)]
    employees: Vec<Input>,
}

fuzz_target!(|data: &[u8]| {
    let Ok(run) = serde_json::from_slice::<Run>(data) else { return };
    let Ok(compiled) = Compiled::new(&run.structure) else { return };
    let _ = compiled.calculation_order();
    for input in run.employees.iter().take(8) {
        let Ok(slip) = compiled.run_explained(input) else { continue };
        assert_eq!(slip.lines.len(), run.structure.components.len());
        let sum = |kind: Kind| slip.lines.iter().filter(|l| l.kind == kind).map(|l| l.amount).sum::<Decimal>();
        assert_eq!(slip.gross, sum(Kind::Earning));
        assert_eq!(slip.employer_cost, sum(Kind::Employer));
        let shortfall: Decimal = slip.lines.iter().map(|l| l.shortfall).sum();
        assert_eq!(slip.shortfall, shortfall);
        assert_eq!(slip.deductions + slip.shortfall, sum(Kind::Deduction));
        assert_eq!(slip.net, slip.gross - slip.deductions);
        let summary = compiled.summarize(input).expect("summarize fails where run succeeded");
        assert_eq!(summary.net, slip.net);
    }
});
