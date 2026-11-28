"""Check the JSON Schemas against every structure, input and payslip in the repository.

    python3 schemas/check.py [demo-output.json]

Needs `jsonschema` (pip install jsonschema). The schemas must stay in step with the Rust types; CI runs this.
"""

import json
import pathlib
import sys

from jsonschema import Draft202012Validator
from referencing import Registry, Resource

ROOT = pathlib.Path(__file__).resolve().parent.parent
schemas = {p.name: json.loads(p.read_text()) for p in (ROOT / "schemas").glob("*.schema.json")}
for schema in schemas.values():
    Draft202012Validator.check_schema(schema)
registry = Registry().with_resources([(s["$id"], Resource.from_contents(s)) for s in schemas.values()])
validators = {name: Draft202012Validator(s, registry=registry) for name, s in schemas.items()}
failures = 0


def check(schema, document, label):
    global failures
    errors = list(validators[schema].iter_errors(document))
    if errors:
        failures += 1
        print(f"FAIL {label}: " + "; ".join(e.message[:200] for e in errors[:3]))


check("run.schema.json", json.loads((ROOT / "example_data/demo_run.json").read_text()), "example_data/demo_run.json")
check("structure.schema.json", json.loads((ROOT / "example_data/stress_structure_10k.json").read_text()), "stress structure")
for number, line in enumerate((ROOT / "example_data/stress_structure_10k_employees.ndjson").read_text().splitlines(), 1):
    if line.strip():
        check("input.schema.json", json.loads(line), f"stress employees line {number}")
for path in sorted((ROOT / "crates/swift_payroll_core/tests/golden").glob("*.json")):
    case = json.loads(path.read_text())
    check("structure.schema.json", case["structure"], f"{path.name} structure")
    for employee in case["employees"]:
        check("input.schema.json", employee, f"{path.name} {employee['employee_id']}")
    for result in case["expected"]:
        check("payslip.schema.json", result, f"{path.name} expected result")
for extra in sys.argv[1:]:
    for result in json.loads(pathlib.Path(extra).read_text()):
        check("payslip.schema.json", result, f"{extra} result")

print(f"{len(schemas)} schemas, {failures} failure(s)")
sys.exit(1 if failures else 0)
