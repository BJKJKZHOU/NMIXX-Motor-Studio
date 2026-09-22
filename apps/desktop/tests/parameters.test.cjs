const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join, resolve } = require("node:path");
const build = process.env.NMIXX_PARAMETER_TEST_BUILD;
if (!build) throw new Error("Run with node scripts/test-parameters.mjs");
const { ParameterDrafts } = require(join(build, "parameters/drafts.js"));
const { parameterText, parseParameterText, equalParameterValue } = require(join(build, "parameters/codec.js"));
const motion = require(join(build, "motion/parameters.js"));
const meta = (typeName, extra = {}) => ({ id: 1, symbol: "PARAM_TEST", label: "Test value", typeName,
  access: "rw", unit: null, description: "", writeState: null, range: null, allowed: [], allowedSymbols: [], ...extra });

for (const type of ["u8", "i8", "u32", "i32", "f32", "position"]) {
  test(`${type}: blank input is not interpreted as zero`, () => assert.throws(() => parseParameterText(meta(type), "  ")));
}
test("integer bounds are enforced before sending IPC", () => {
  for (const [type, min, max] of [["u8", 0, 255], ["i8", -128, 127], ["u32", 0, 4294967295], ["i32", -2147483648, 2147483647]]) {
    assert.equal(parseParameterText(meta(type), String(min)).value, min);
    assert.equal(parseParameterText(meta(type), String(max)).value, max);
    for (const bad of [min - 1, max + 1, 1.5]) assert.throws(() => parseParameterText(meta(type), String(bad)));
  }
});
test("f32 rejects nonfinite and overflow values", () => {
  for (const bad of ["NaN", "Infinity", "-Infinity", "1e100"]) assert.throws(() => parseParameterText(meta("f32"), bad));
  assert.deepEqual(parseParameterText(meta("f32"), " 1.25 "), { type: "f32", value: 1.25 });
});
test("position tuple parsing rejects missing components and turn overflow", () => {
  assert.deepEqual(parseParameterText(meta("position"), "-2, 1.5"), { type: "position", value: { turns: -2, theta: 1.5 } });
  for (const bad of ["1", "1,", ",1", "1,2,3", "2147483648,0", "0,1e100"]) assert.throws(() => parseParameterText(meta("position"), bad));
});
test("canonical equality does not merge different types or small committed changes", () => {
  assert(equalParameterValue(null, null));
  assert(equalParameterValue({ type: "u8", value: 1 }, { type: "u8", value: 1 }));
  assert(!equalParameterValue({ type: "u8", value: 1 }, { type: "f32", value: 1 }));
  assert(!equalParameterValue({ type: "f32", value: 1e-7 }, { type: "f32", value: 2e-7 }));
  assert(!equalParameterValue({ type: "position", value: { turns: 0, theta: 1 } }, { type: "position", value: { turns: 1, theta: 1 } }));
});
test("missing values remain empty rather than fabricated zero", () => {
  assert.equal(parameterText(null), "");
  assert.equal(motion.motionParameterText(meta("f32"), undefined), "");
});
test("untouched fields follow the latest shared committed value", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1);
  assert.equal(drafts.text("kp", "10"), "10");
  assert.equal(drafts.text("kp", "20"), "20");
  assert(!drafts.dirty("kp", "20"));
});
test("unrelated telemetry updates cannot overwrite an edited field", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "125.");
  for (let i = 0; i < 100; i++) {
    assert.equal(drafts.text("vbus", String(i)), String(i));
    assert.equal(drafts.text("kp", "10"), "125.");
  }
});
test("same-parameter external update preserves a draft until discard", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "125");
  assert.equal(drafts.text("kp", "20"), "125");
  assert(drafts.dirty("kp", "20"));
  drafts.discard("kp");
  assert.equal(drafts.text("kp", "20"), "20");
});
test("blur or Escape discards only the selected draft", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "11"); drafts.edit("ki", "12");
  drafts.discard("kp");
  assert.equal(drafts.text("kp", "10"), "10");
  assert.equal(drafts.text("ki", "20"), "12");
});
test("Enter captures text before blur and canonical readback", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "12.5");
  const submitted = drafts.text("kp", "10");
  drafts.discard("kp");
  assert.equal(submitted, "12.5");
  assert.equal(drafts.text("kp", "12.50001"), "12.50001");
});
test("paired acceleration/deceleration commits retain their captured value", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1);
  drafts.edit("acc", "200"); drafts.edit("dec", "200");
  const first = drafts.text("acc", "100"); const second = first;
  drafts.discard("acc"); drafts.discard("dec");
  assert.equal(first, "200"); assert.equal(second, "200");
});
test("connection change clears drafts, same-connection refresh does not", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "12");
  drafts.resetForConnection(1); assert.equal(drafts.text("kp", "10"), "12");
  drafts.resetForConnection(2); assert.equal(drafts.text("kp", "50"), "50");
});
test("a draft equal to the new committed value is not dirty", () => {
  const drafts = new ParameterDrafts(); drafts.resetForConnection(1); drafts.edit("kp", "20");
  assert(drafts.dirty("kp", "10")); assert(!drafts.dirty("kp", "20"));
});
test("Motion decimal turns preserve negative positions and range checks", () => {
  for (const turns of [-2.25, -0.1, 0, 1.75, 123.125]) assert(Math.abs(motion.positionToTurns(motion.turnsToPosition(turns)) - turns) < 1e-9);
  for (const bad of [NaN, Infinity, -2147483649, 2147483648]) assert.throws(() => motion.turnsToPosition(bad));
  assert.throws(() => motion.parseMotionParameter(meta("position"), ""));
});
test("Motion mode selection uses schema enum values rather than assumed ordinals", () => {
  const definition = meta("u8", { allowedSymbols: ["TORQUE", "POSITION", "SPEED"], allowed: [7, 12, 21] });
  assert.equal(motion.modeParameterValue(definition, "position"), 12);
  assert.equal(motion.modeFromParameter(definition, { type: "u8", value: 21 }), "speed");
  assert.equal(motion.modeParameterValue(definition, "sensorless-speed"), undefined);
});

const desktop = resolve(__dirname, "..");
for (const page of ["motor/MotorPage", "encoder/EncoderPage", "limits/LimitsPage", "control/ControlPage", "control/ControlTuningPage", "motion/MotionPage", "parameters/ParameterTablePage"]) {
  test(`${page}: parameters come from shared view/editor, not page I/O`, () => {
    const source = readFileSync(join(desktop, "src", `${page}.svelte`), "utf8");
    assert.match(source, /selectParameters\(/);
    assert.match(source, /createParameterEditor\(/);
    assert.doesNotMatch(source, /\b(?:readParameter|readParameters|readCachedParameters|readCurrentParameters|writeParameter|listParameters|onParametersChanged|onParametersRefreshed|refreshFromCache)\s*\(/);
    assert.doesNotMatch(source, /let\s+values\s*=\s*\$state/);
    assert.match(source, /onblur=/);
  });
}
test("Motion preview implementation uses a cache snapshot, not device reads", () => {
  const source = readFileSync(resolve(desktop, "../../crates/nmixx-app/src/motion.rs"), "utf8");
  const preview = source.slice(source.indexOf("pub fn preview_with_parameters"), source.indexOf("pub(crate) fn run"));
  assert.match(preview, /parameters\.snapshot\(\)/);
  assert.doesNotMatch(preview, /parameters\.read\(/);
});
test("dependency readback and changed notification are shared backend responsibilities", () => {
  const source = readFileSync(resolve(desktop, "../../crates/nmixx-app/src/parameter_service.rs"), "utf8");
  assert.match(source, /fn readback_ids/);
  assert.match(source, /self\.readback_ids\(id\)/);
  assert.match(source, /cache\.get\(&id\) != Some\(&entry\)/);
  assert.match(source, /ReadbackFailed/);
});
