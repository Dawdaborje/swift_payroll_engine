run-tests:
    cargo test -- --nocapture

# Criterion suite (includes 1.5M stream_parallel).
bench:
    cargo bench -p swift_payroll_engine --bench payroll_throughput

bench-once n="1500000":
    cargo run -p swift_payroll_engine --release --bin bench_once -- {{n}}
