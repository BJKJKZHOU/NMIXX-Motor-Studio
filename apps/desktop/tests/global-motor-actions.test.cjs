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
  assert.match(source,/Motor \{motorState \?\? "unknown"\}/);
});
test("device Stop rejection appears as an error and does not claim success",async()=>{
  const h=harness();
  h.stopFails(new Error("ERR_STATE"));
  await h.stop();
  assert.deepEqual(h.operations,["ACTION_MOTOR_STOP"]);
  assert.match(h.read("motorActionNotice"),/Stop command feedback.*ERR_STATE/);
  assert.equal(h.errors.length,1);
});
