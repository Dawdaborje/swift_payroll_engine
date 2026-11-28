import json
from decimal import Decimal
from pathlib import Path

import pytest

from swift_payroll_engine import Payroll

DEMO = json.loads((Path(__file__).parents[3] / "example_data" / "demo_run.json").read_text(encoding="utf-8"))


def test_demo_run_matches_cli():
    results = Payroll(DEMO["structure"]).run_all(DEMO["employees"])
    assert [r.get("net") for r in results[:2]] == ["54800.00", "116100.46"]
    assert results[2]["error"] == "E-003: `base_salary` is missing"


def test_run_raises_for_a_bad_employee():
    with pytest.raises(ValueError, match="E-003"):
        Payroll(DEMO["structure"]).run(DEMO["employees"][2])


def test_bad_structure_raises():
    with pytest.raises(ValueError):
        Payroll({"id": "x", "version": "1", "currency": "GMD", "components": [{"id": "a", "label": "A", "kind": "earning", "formula": "nope"}]})


def test_decimal_input_is_exact_and_duplicates_refused():
    employee = dict(DEMO["employees"][0], variables=dict(DEMO["employees"][0]["variables"], base_salary=Decimal("50000")))
    results = Payroll(json.dumps(DEMO["structure"])).run_all([employee, employee])
    assert results[0]["net"] == "54800.00"
    assert "more than once" in results[1]["error"]


def test_empty_employee_id_refused():
    results = Payroll(DEMO["structure"]).run_all([dict(DEMO["employees"][0], employee_id="")])
    assert results[0]["error"] == "employee_id is empty"


def test_decimal_input_is_a_number_in_formulas():
    structure = {"id": "x", "version": "1", "currency": "GMD", "inputs": [{"name": "salary"}],
                 "components": [{"id": "pay", "label": "Pay", "kind": "earning", "formula": "salary * 2"}]}
    slip = Payroll(structure).run({"employee_id": "e1", "variables": {"salary": Decimal("1234567890123456.78")}})
    assert slip["gross"] == "2469135780246913.56"
