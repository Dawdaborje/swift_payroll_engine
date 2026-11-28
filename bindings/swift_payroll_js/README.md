# swift-payroll-engine (JavaScript / TypeScript)

Exact-decimal payslips from a JSON salary structure, using the Rust Swift payroll engine compiled to WebAssembly.

```ts
import { Payroll } from "swift-payroll-engine";

const payroll = new Payroll(structure);   // object or JSON text; throws if invalid
const slip = payroll.run(employee);       // one payslip; throws on error
const results = payroll.runAll(employees); // one payslip or { employee_id, error } per employee
```

Amounts come back as decimal strings (`"54800.00"`). A JS number is a 64-bit float, exact only to about 15
significant digits, so an amount passed in an object has already lost any digits past that before the engine
sees it. Pass such amounts as decimal strings (`"base_salary": "1234567890123456.78"`) to inputs declared
`"type": "number"`, or pass the whole input as JSON text: numbers in JSON text are read exactly.

Input schema: [docs/data-format.md](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/data-format.md).
More in [bindings/README.md](../README.md).
