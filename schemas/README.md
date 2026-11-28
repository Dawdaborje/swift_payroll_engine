# JSON Schemas

Machine-readable versions of the formats in [docs/data-format.md](../docs/data-format.md), in JSON Schema
draft 2020-12.

| Schema | What it describes |
|---|---|
| [`structure.schema.json`](./structure.schema.json) | A salary structure. |
| [`input.schema.json`](./input.schema.json) | One person: an element of `employees`, or one line of an NDJSON employee file. |
| [`run.schema.json`](./run.schema.json) | The file given to `--input`: a structure and its employees. |
| [`payslip.schema.json`](./payslip.schema.json) | One result printed by the command line: a payslip, a `--summary` record, or an error. |

Point an editor at a schema by adding `"$schema"` to a structure (the engine ignores the field):

```json
{ "$schema": "https://raw.githubusercontent.com/Dawdaborje/swift_payroll_engine/main/schemas/structure.schema.json", "id": "demo", ... }
```

A schema catches typos, wrong types and missing fields. It cannot check what only the compiler can: that names
are unique across inputs, constants and components, that every reference exists, that there is no cycle, and
that bracket tables are well formed. A structure that passes the schema can still be refused by `Compiled::new`.

The rule pack schema lives with the packs, in
[swift_tax_rules](https://github.com/borjes-tech/swift_tax_rules/tree/main/schemas).

`python3 schemas/check.py` validates every structure, input and recorded payslip in this repository against
these schemas; CI runs it.
