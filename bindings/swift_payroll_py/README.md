# swift-payroll-engine (Python)

Exact-decimal payslips from a JSON salary structure, using the Rust Swift payroll engine.

```python
from swift_payroll_engine import Payroll

payroll = Payroll(structure)          # dict or JSON text; raises ValueError if invalid
slip = payroll.run(employee)          # one payslip; raises ValueError on error
results = payroll.run_all(employees)  # one payslip or {"employee_id", "error"} per employee
```

Input schema: [docs/data-format.md](https://github.com/Dawdaborje/swift_payroll_engine/blob/main/docs/data-format.md).
More in [bindings/README.md](../README.md).
