import { test } from "node:test";
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { createRequire } from "node:module";

const { Payroll } = createRequire(import.meta.url)("../pkg/swift_payroll_engine.js");
const demo = JSON.parse(readFileSync(new URL("../../../example_data/demo_run.json", import.meta.url), "utf8"));

test("demo run matches the CLI", () => {
  const results = new Payroll(demo.structure).runAll(demo.employees);
  assert.deepEqual(results.slice(0, 2).map((r) => r.net), ["54800.00", "116100.46"]);
  assert.equal(results[2].error, "E-003: `base_salary` is missing");
});

test("run throws for a bad employee", () => {
  assert.throws(() => new Payroll(demo.structure).run(demo.employees[2]), /E-003/);
});

test("bad structure throws", () => {
  const bad = { id: "x", version: "1", currency: "GMD", components: [{ id: "a", label: "A", kind: "earning", formula: "nope" }] };
  assert.throws(() => new Payroll(bad));
});

test("JSON text input and repeated employees", () => {
  const results = new Payroll(JSON.stringify(demo.structure)).runAll(JSON.stringify([demo.employees[0], demo.employees[0]]));
  assert.equal(results[0].net, "54800.00");
  assert.match(results[1].error, /more than once/);
});

test("a missing or unwritable argument says what was expected", () => {
  assert.throws(() => new Payroll(), /expected an object/);
  assert.throws(() => new Payroll(demo.structure).run(() => 1), /expected an object/);
});
