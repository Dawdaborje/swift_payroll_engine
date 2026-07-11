# sp_dsl

Rule compilation and evaluation for Swift Payroll Engine.

## Supported engines

| DSL | Status | Context |
|-----|--------|---------|
| **CEL** | Implemented | Variable `gross` (f64) |
| **Rhai** | Implemented | Variable `gross` (f64) |
| Odoo HRMS | Planned | `UnsupportedDsl` |
| Frappe / ERPNext | Planned | `UnsupportedDsl` |

## Usage

```rust
use sp_dsl::models::{DslType, PayrollRuleContext};

let rule = PayrollRuleContext::new(
    "PAYE".into(),
    "Income Tax".into(),
    "gross <= 10000 ? 0.0 : (gross - 10000) * 0.15".into(),
    DslType::CEL,
)?;

let amount = rule.evaluate(gross_decimal)?;
```

Rules are compiled through `RuleCache::global()` and cached per OS thread on first evaluation.

See [technical docs](../../docs/technical/README.md#rule-engines) for caching details.
