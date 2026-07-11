use std::env;
use std::time::Instant;

use sp_dsl::RuleCache;
use sp_engine::calculator::CalculationContext;
use swift_payroll::cohort::ProfileCohort;
use swift_payroll::fixture::{example_data_root, load_employees};

fn main() {
    let n: usize = env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1_500_000);

    let path = example_data_root().join("benchmark/profiles.json");
    let profiles = load_employees(&path).expect("load profiles");
    let cohort = ProfileCohort::from_profiles(&profiles).expect("build cohort");

    eprintln!(
        "streaming {} templates over {n} employees",
        cohort.template_count()
    );

    let cache = RuleCache::global();
    eprintln!("compiled rules in cache: {}", cache.len());

    let t0 = Instant::now();
    let processed = CalculationContext::calculate_stream_count(n, |i| cohort.employee_at(i))
        .expect("calculate");
    let calc_secs = t0.elapsed().as_secs_f64();
    assert_eq!(processed, n);

    eprintln!(
        "parallel calculate: {n} employees in {calc_secs:.3}s ({:.0} emp/s)",
        n as f64 / calc_secs
    );
}
