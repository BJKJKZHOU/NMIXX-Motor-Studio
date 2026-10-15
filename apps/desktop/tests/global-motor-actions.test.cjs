// Global operator controls, using actual App.svelte commands and fake application Action results.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");

const source = readFileSync(join(__dirname, "../src/App.svelte"), "utf8");
function command(name) {
  const match = source.match(new RegExp(`^  async function ${name}\\(\\) \\{[\\s\\S]*?^  \\}`, "m"));
  assert.ok(match, `Expected ${name} in the actual global toolbar`);
  return match[0];
}
function harness() {
  const operations = [], errors = [];
  let stopError;
  const context = vm.createContext({
    connection: {port:"test"}, connectionReady:true, motorState:"DISABLED",
    globalActionBusy:false, globalStopBusy:false, motorActionNotice:"",
    setError:error=>errors.push(String(error)),
    enableMotor:async()=>operations.push("ACTION_MOTOR_ENABLE"),
    disableMotor:async()=>operations.push("ACTION_MOTOR_DISABLE"),
    stopMotor:async()=>{operations.push("ACTION_MOTOR_STOP");if(stopError)throw stopError;},
  });
  vm.runInContext([command("toggleMotorEnable"),command("stopCurrentMotorOperation")].join("\n"),context);
  const evalState=expr=>vm.runInContext(expr,context);
  return {operations,errors,read:evalState,write:evalState,
    enable:()=>evalState("toggleMotorEnable()"),
    stop:()=>evalState("stopCurrentMotorOperation()"),
    stopFails:error=>stopError=error};
}
test("Enable and Disable have distinct device commands and accepted feedback",async()=>{
  const h=harness();
  await h.enable();
  assert.deepEqual(h.operations,["ACTION_MOTOR_ENABLE"]);
  assert.match(h.read("motorActionNotice"),/Enable accepted/);
  h.write('motorState = "ENABLED"');
  await h.enable();
  assert.deepEqual(h.operations,["ACTION_MOTOR_ENABLE","ACTION_MOTOR_DISABLE"]);
  assert.match(h.read("motorActionNotice"),/Disable accepted/);
});
test("Stop acknowledgement never falsely claims controlled motion has finished",async()=>{
  const h=harness();
  h.write('motorState = "RUN"');
  await h.stop();
  assert.deepEqual(h.operations,["ACTION_MOTOR_STOP"]);
  assert.match(h.read("motorActionNotice"),/Stop accepted; monitor firmware Motor State/);
  assert.equal(h.read("motorState"),"RUN");
  assert.doesNotMatch(h.read("motorActionNotice"),/motor stopped/i);
  assert.match(source,/translate\("ui\.motor", \$uiLanguage\)\} \{motorState \?\? "unknown"\}/);
});
test("device Stop rejection appears as an error and does not claim success",async()=>{
  const h=harness();
  h.stopFails(new Error("ERR_STATE"));
  await h.stop();
  assert.deepEqual(h.operations,["ACTION_MOTOR_STOP"]);
  assert.match(h.read("motorActionNotice"),/Stop command feedback.*ERR_STATE/);
  assert.equal(h.errors.length,1);
});

test("global power button shows firmware enable state, not the next Action", () => {
  const ts = require("typescript");
  const motorStateSource = readFileSync(join(__dirname, "../src/motor/state.ts"), "utf8");
  const module = { exports: {} };
  const code = ts.transpileModule(motorStateSource, {
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.CommonJS }
  }).outputText;
  vm.runInNewContext(code, {
    module, exports: module.exports,
    require: () => ({ parameterEnumSymbol: () => undefined }),
  });
  const label = module.exports.motorEnableStatus;
  assert.equal(label("DISABLED", true), "DISABLED");
  assert.equal(label("ENABLED", true), "ENABLED");
  assert.equal(label("RUN", true), "ENABLED");
  assert.equal(label(null, true), "UNKNOWN");
  assert.equal(label("ENABLED", false), "UNKNOWN");
  assert.match(source, /class:motor-disabled=\{enableStatus === "DISABLED"\}/);
  assert.match(source, /class:motor-enabled=\{enableStatus === "ENABLED"\}/);
  assert.match(source, /<i class="codicon codicon-power"><\/i> \{enableStatus\}/);
  assert.match(source, /\.motor-disabled \{ color: #f85149; \}/);
  assert.match(source, /\.motor-enabled \{ color: #3fb950; \}/);
  assert.match(source, /title=\{localize\(enableStatus === "DISABLED" \? "Motor DISABLED — click to Enable"/);
});

test("global power button changes only after device Motor State readback", async () => {
  const h = harness();
  const label = (state) => state === "DISABLED" ? "DISABLED" : "ENABLED";
  assert.equal(label(h.read("motorState")), "DISABLED");
  await h.enable();
  assert.deepEqual(h.operations, ["ACTION_MOTOR_ENABLE"]);
  // Action acceptance alone must not update the device-owned Motor State.
  assert.equal(label(h.read("motorState")), "DISABLED");
  h.write('motorState = "ENABLED"'); // next authoritative Parameter update
  assert.equal(label(h.read("motorState")), "ENABLED");
  h.write('motorState = "RUN"');
  assert.equal(label(h.read("motorState")), "ENABLED");
  await h.enable();
  assert.deepEqual(h.operations, ["ACTION_MOTOR_ENABLE", "ACTION_MOTOR_DISABLE"]);
});
