# Bindings

Python and JavaScript/TypeScript packages for Swift Payroll Engine, both named `swift-payroll-engine`. Neither is
published yet; install from this repository as shown below.

Both have the same API: `Payroll(structure)` compiles a structure once (and throws/raises if it is invalid), then
`run(employee)` gives one payslip and `run_all(employees)` / `runAll(employees)` gives one result per employee,
a payslip or `{ "employee_id", "error" }`, exactly like the CLI. Pass `explain=True` / `true` for explained lines.
Input and output shapes: [docs/data-format.md](../docs/data-format.md) and [schemas/](../schemas/). Amounts come
back as decimal strings (`"54800.00"`).

Both crates are outside the Cargo workspace, each with its own `Cargo.lock`, and carry the workspace version
by hand: bump `version` in `swift_payroll_py/Cargo.toml` and `swift_payroll_js/Cargo.toml` with each release.

## Python (`swift_payroll_py`)

PyO3 + maturin, one abi3 wheel for Python 3.9 and newer. Installing from source needs a Rust toolchain.

```sh
pip install "git+https://github.com/Dawdaborje/swift_payroll_engine#subdirectory=bindings/swift_payroll_py"
```

```python
from swift_payroll_engine import Payroll

payroll = Payroll(structure)          # dict or JSON text
slip = payroll.run({"employee_id": "e1", "variables": {"base_salary": Decimal("50000")}})
results = payroll.run_all(employees)
```

`Decimal` values are passed as text, so they are read exactly. Tests: `just py-test`.

## JavaScript / TypeScript (`swift_payroll_js`)

`wasm-bindgen` + `wasm-pack`, built for Node (`--target nodejs`); types are generated. Build and pack it, then
install the tarball in another project:

```sh
cd bindings/swift_payroll_js && wasm-pack build --target nodejs --release && npm pack ./pkg
npm install /path/to/swift-payroll-engine-0.3.0.tgz
```

```ts
import { Payroll } from "swift-payroll-engine";

const payroll = new Payroll(structure);   // object or JSON text
const results = payroll.runAll(employees);
```

Pass amounts above 2^53 as strings (`"123456789012345.67"`); a JS number loses digits before the engine sees it.
Tests: `just js-test`.
