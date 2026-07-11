use std::time::Duration;

use criterion::{BenchmarkId, Criterion, Throughput, criterion_group, criterion_main};
use sp_engine::calculator::CalculationContext;
use swift_payroll::cohort::ProfileCohort;
use swift_payroll::fixture::{example_data_root, load_employees};

fn payroll_throughput(c: &mut Criterion) {
    let path = example_data_root().join("benchmark/profiles.json");
    let profiles = load_employees(&path).expect("load benchmark profiles");
    let cohort = ProfileCohort::from_profiles(&profiles).expect("build cohort");

    let mut group = c.benchmark_group("payroll_calculate");

    for &n in &[100usize, 1_000, 10_000] {
        group.sample_size(10);
        group.warm_up_time(Duration::from_secs(2));
        group.measurement_time(Duration::from_secs(20));
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("stream_parallel", n), &cohort, |b, cohort| {
            b.iter(|| {
                CalculationContext::calculate_stream_count(n, |i| cohort.employee_at(i))
                    .expect("stream parallel")
            })
        });
        group.bench_with_input(
            BenchmarkId::new("stream_sequential", n),
            &cohort,
            |b, cohort| {
                b.iter(|| {
                    CalculationContext::calculate_stream_sequential_count(n, |i| {
                        cohort.employee_at(i)
                    })
                    .expect("stream sequential")
                })
            },
        );
    }

    for &n in &[100_000usize, 1_500_000] {
        group.sample_size(10);
        group.warm_up_time(Duration::from_secs(1));
        group.measurement_time(Duration::from_secs(if n >= 1_500_000 { 180 } else { 40 }));
        group.throughput(Throughput::Elements(n as u64));

        group.bench_with_input(BenchmarkId::new("stream_parallel", n), &cohort, |b, cohort| {
            b.iter(|| {
                CalculationContext::calculate_stream_count(n, |i| cohort.employee_at(i))
                    .expect("stream parallel")
            })
        });
    }

    group.finish();
}

criterion_group!(benches, payroll_throughput);
criterion_main!(benches);
