#!/usr/bin/env python3
"""The calculation model of Odoo's hr_payroll, as a best case, on the same structure and people.

Odoo evaluates a payslip by walking the salary rules in sequence; each rule has a Python condition and a
Python amount, evaluated against a dict of everything calculated so far (`localdict`), and the result is
stored back in it. This does exactly that with floats (Odoo's amounts are floats), with every rule compiled
once up front. What it leaves out makes it FASTER than real Odoo: `safe_eval`'s opcode check on every
evaluation, the ORM (browse records, computed fields, prefetch), reading worked days and inputs from the
database, and writing payslip line records. So these numbers are a floor for Odoo, not an estimate of it.

    python3 odoo_style_baseline.py structure.json people.ndjson [people]
"""
import json, sys, time

def pct(x, p): return x * p / 100
def clamp(x, lo, hi): return max(lo, min(hi, x))
def prorate(a, part, whole): return a * part / whole
def bracket(x, table):
    if x <= 0: return 0.0
    tax, lower = 0.0, 0.0
    for upper, rate in table:
        top = x if upper is None else min(upper, x)
        if top > lower: tax += (top - lower) * rate
        if upper is not None and x > upper: lower = upper
        else: break
    return tax

def to_python(formula):
    if " ? " in formula:
        cond, rest = formula.split(" ? ", 1)
        a, b = rest.split(" : ", 1)
        return f"({a}) if ({cond}) else ({b})"
    return formula

def main():
    structure = json.load(open(sys.argv[1]))
    lines = [l for l in open(sys.argv[2]) if l.strip()]
    count = int(sys.argv[3]) if len(sys.argv) > 3 else len(lines)
    people = [json.loads(l) for l in lines[:count]]
    defaults = {i["name"]: i.get("default") for i in structure["inputs"]}
    constants = {k: [[u, r] for u, r in v] if isinstance(v, list) else v for k, v in structure["constants"].items()}
    rules = []
    for c in structure["components"]:
        if "formula" in c: code = compile(to_python(c["formula"].replace("total(\"", "total_of(\"")), c["id"], "eval")
        else: code = compile(c["input"], c["id"], "eval")
        when = compile(c["when"], c["id"] + ".when", "eval") if c.get("when") else None
        rules.append((c["id"], c["kind"], [c["kind"]] + c.get("tags", []), code, when))

    def one(person):
        local = dict(constants)
        for k, d in defaults.items():
            local[k] = person["variables"].get(k, d)
            if local[k] is None: raise ValueError(f"{k} is missing")
        sums = {}
        local["total_of"] = lambda tag: sums.get(tag, 0.0)
        local.update(pct=pct, clamp=clamp, prorate=prorate, bracket=bracket, min=min, max=max)
        gross = ded = 0.0
        for rid, kind, tags, code, when in rules:
            if when is not None and not eval(when, local):
                local[rid] = 0.0
                continue
            amount = round(float(eval(code, local)), 2)
            local[rid] = amount
            for t in tags: sums[t] = sums.get(t, 0.0) + amount
            if kind == "earning": gross += amount
            elif kind == "deduction": ded += amount
        return round(gross - ded, 2)

    nets = {}
    start = time.perf_counter()
    for p in people:
        try: nets[p["employee_id"]] = one(p)
        except Exception: pass
    elapsed = time.perf_counter() - start
    print(json.dumps({"components": len(rules), "people": len(people), "calculated": len(nets),
                      "seconds": round(elapsed, 3), "per_payslip_us": round(elapsed / len(people) * 1e6, 1),
                      "payslips_per_s": round(len(people) / elapsed)}))
    json.dump(nets, open(sys.argv[2] + ".odoo_nets.json", "w"))

main()
