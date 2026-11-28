run-tests:
    cargo test -- --nocapture

# Scale test: employees x CEL rules, checked against an independent oracle (see example_data/README.md).
stress employees="100000" rules="10000":
    cargo run -p swift_payroll_engine --release --example stress -- run --employees {{employees}} --rules {{rules}}

# The full 1,000,000 x 10,000 run (about 9 minutes on 16 cores).
stress-1m:
    cargo run -p swift_payroll_engine --release --example stress -- run --employees 1000000 --rules 10000 --oracle-every 1000

# Property tests and fuzzers at scale (the default `cargo test` runs a quick pass of each).
fuzz iters="2000000":
    FUZZ_ITERS={{iters}} cargo test --release -p swift_payroll_expr --test fuzz
    FUZZ_ITERS={{iters}} cargo test --release -p swift_payroll_core --test fuzz

# Python bindings: build into a local venv and run pytest (needs uv).
py-test:
    cd bindings/swift_payroll_py && uv venv -q --allow-existing .venv && uv pip install -q -p .venv --reinstall ".[test]" && .venv/bin/pytest -q tests

# JS/TS bindings: build the wasm package and run the node tests (needs wasm-pack).
js-test:
    cd bindings/swift_payroll_js && wasm-pack build --target nodejs --release && node --test tests/
