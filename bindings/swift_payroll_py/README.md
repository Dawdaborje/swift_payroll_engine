# Swift Payroll — Python bindings

**Not implemented yet.** This directory is a placeholder for a possible future Python package. The current
`pyproject.toml` has no build backend, dependencies, or native extension module, so this package cannot be
installed or imported as `swift_payroll_engine` yet.

Possible future usage (the API is not specified):

```python
import swift_payroll_engine

results = swift_payroll_engine.calculate(structure, employees)  # one result per employee
```

Input schema: [docs/data-format.md](../../docs/data-format.md).
