"""Python bindings for the Swift payroll engine. Input shape: docs/data-format.md in the repository.

Amounts come back as decimal text ("54800.00"); Decimal(slip["net"]) gives the exact value.
"""

import json
import re
import uuid
from decimal import Decimal
from typing import Any, List, Union

from . import _native

__all__ = ["Payroll"]


def _text(value: Any) -> str:
    # A finite Decimal is written as a JSON number, digit for digit (the native module reads JSON numbers
    # exactly), so formulas see a number, not text. Anything else json can't write goes in as text.
    if isinstance(value, str):
        return value
    marker = uuid.uuid4().hex
    numbers: List[str] = []

    def default(o: Any) -> Any:
        if isinstance(o, Decimal) and o.is_finite():
            numbers.append(str(o))
            return f"{marker}{len(numbers) - 1}"
        return str(o)

    text = json.dumps(value, default=default)
    return re.sub(f'"{marker}(\\d+)"', lambda m: numbers[int(m.group(1))], text) if numbers else text


class Payroll:
    """A salary structure, compiled once. Raises ValueError if the structure is invalid."""

    def __init__(self, structure: Union[dict, str]):
        self._native = _native.Payroll(_text(structure))

    @property
    def warnings(self) -> List[str]:
        return self._native.warnings

    def run(self, employee: Union[dict, str], explain: bool = False) -> dict:
        """One payslip. Raises ValueError if this employee can't be calculated."""
        return json.loads(self._native.run(_text(employee), explain))

    def run_all(self, employees: Union[list, str], explain: bool = False) -> List[dict]:
        """One result per employee: a payslip, or {"employee_id", "error"}. Never raises for one bad employee."""
        return json.loads(self._native.run_all(_text(employees), explain))

