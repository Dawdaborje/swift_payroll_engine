# Benchmark data

| File | Role |
|------|------|
| [`profiles.json`](profiles.json) | Compact templates (basic / intermediate / advanced). Each has **10–100 tax** deductions (CEL + Rhai), plus one fixed loan. |
| [`cohort_500.json`](cohort_500.json) | Expanded sample (500 employees) for integration stress tests. Same schema as other `example_data` files; no `results` field. |

## Why there is no 1M JSON in the repo

One million employees are generated on the fly from `profiles.json` via `ProfileCohort` and calculated with `CalculationContext::calculate_stream_count` (no full cohort or result vectors).

```bash
cargo bench -p swift_payroll_engine --bench payroll_throughput
just bench
just bench-once
just bench-once 1000
just bench-once 1500000
```
