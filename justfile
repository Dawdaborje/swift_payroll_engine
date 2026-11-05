run-tests:
    cargo test -- --nocapture

# Criterion suite (includes 1.5M stream_parallel).
bench:
    cargo bench -p swift_payroll_engine --bench payroll_throughput

bench-once n="1500000":
    cargo run -p swift_payroll_engine --release --bin bench_once -- {{n}}

# Scale test: employees x CEL rules, checked against an independent oracle (see example_data/README.md).
stress employees="100000" rules="10000":
    cargo run -p swift_payroll_engine --release --example stress -- run --employees {{employees}} --rules {{rules}}

# The full 1,000,000 x 10,000 run (about 9 minutes on 16 cores).
stress-1m:
    cargo run -p swift_payroll_engine --release --example stress -- run --employees 1000000 --rules 10000 --oracle-every 1000

# Property tests and fuzzers at scale (the default `cargo test` runs a quick pass of each).
fuzz iters="2000000":
    FUZZ_ITERS={{iters}} cargo test --release -p sp_expr --test fuzz
    FUZZ_ITERS={{iters}} cargo test --release -p sp_engine --test fuzz
