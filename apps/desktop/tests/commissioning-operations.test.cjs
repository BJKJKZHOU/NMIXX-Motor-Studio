// Execute the real Motor / Encoder page event handlers with mocked Tauri results.
// Hardware is not simulated: these tests assert the user-facing state transitions
// and semantic IPC inputs while protocol frame tests cover the Session boundary.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");

const desktop = join(__dirname, "..");
function source(name) { return readFileSync(join(desktop, "src", name), "utf8"); }
function extract(src, name) {
  const beginning = src.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*`, "m"));
  assert.ok(beginning, `Expected actual ${name} user handler`);
  if (beginning[0].trimEnd().endsWith("}")) return beginning[0];
  const match = src.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*\\{[\\s\\S]*?^  \\}`, "m"));
  assert.ok(match, `Expected complete ${name} user handler`);
  return match[0];
}
function compile(source) {
  return ts.transpileModule(source, {compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
}
function motorHarness() {
  const page = source("motor/MotorPage.svelte");
  const config = page.match(/^  const IDENT_CONFIGS: Record<IdentKey, IdentConfig> = \{[\s\S]*?^  \};/m);
  assert.ok(config, "Motor identification configuration is required");
  const functions = ["handleKey","numericValue","identificationAvailable","setIdentState",
    "identifyBusy","startIdentification","applyIdentification","handleActionCompleted"];
  let initial = config[0] + "\n" + functions.map(name => extract(page,name)).join("\n");
  let response = { status: "started", handle: {txn: 1,actionId:0x1101} };
  let confirmResult = false;
  const calls = [], errors = [];
  const context = vm.createContext({
    connection:{commissioning:{identificationRsLs:true,identificationFlux:true,identificationJb:true,identificationApply:true}},
    identStates:{rsLs:{phase:"idle",message:""},flux:{phase:"idle",message:""},jb:{phase:"idle",message:""}},
    generation:0,startingIdent:null,earlyCompletion:undefined,pendingHandles:{},
    applyCandidate:null,failReasonSymbol:"PARAM_IDENT_FAIL_REASON",
    values:{PARAM_IDENT_RS_LS_VALID:{type:"u8",value:1}},
    onError:(error)=>errors.push(String(error)),
    window:{confirm:(text)=>{calls.push(["confirm",text]);return confirmResult;}},
    startIdentificationAction:async (key,allowEnable)=>{calls.push(["identification_start",key,allowEnable]);return typeof response==="function"?response(key,allowEnable):response;},
    applyIdentificationAction:async (key)=>{calls.push(["identification_apply",key]);return {txn:2,actionId:0x1104};},
  });
  vm.runInContext(compile(initial),context);
  const get=(src)=>vm.runInContext(src,context);
  return {calls,errors,get,run:(key="rsLs")=>get(`startIdentification("${key}")`),
    complete:(event)=>{context.completed=event;return get("handleActionCompleted(completed)")},
    apply:(key="rsLs")=>get(`applyIdentification("${key}")`),
    setResponse:(value)=>response=value,setConfirm:(value)=>confirmResult=value};
}
test("Motor Rs/Ls: user Start -> Action accepted -> Running -> valid completion -> Apply -> Applied",async()=>{
  const h=motorHarness();
  await h.run();
  assert.equal(h.calls[0][0],"identification_start");
  assert.equal(h.calls[0][1],"rsLs");
  assert.equal(h.calls[0][2],false);
  assert.equal(h.get("identStates.rsLs.phase"),"running");
  h.complete({txn:1,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true});
  assert.equal(h.get("identStates.rsLs.phase"),"ready");
  assert.equal(h.get("applyCandidate"),"rsLs");
  await h.apply();
  assert.deepEqual(h.calls.at(-1),["identification_apply","rsLs"]);
  assert.equal(h.get("identStates.rsLs.phase"),"applied");
});

test("Motor blocked preflight shows reasons without proposing false Running",async()=>{
  const h=motorHarness();
  h.setResponse({status:"blocked",issues:[{reason:"Encoder not valid"}]});
  await h.run();
  assert.equal(h.get("identStates.rsLs.phase"),"failed");
  assert.match(h.get("identStates.rsLs.message"),/Encoder not valid/);
  assert.equal(h.get("startingIdent"),null);
});

test("Motor identification explicit Enable confirmation changes request flag only after approval",async()=>{
  const h=motorHarness();
  h.setResponse((key,allowEnable)=>allowEnable?{status:"started",handle:{txn:3,actionId:0x1101}}:{status:"requires_enable"});
  h.setConfirm(false);
  await h.run();
  assert.equal(h.calls.filter(c=>c[0]==="identification_start").length,1);
  h.setConfirm(true);
  await h.run();
  assert.deepEqual(h.calls.filter(c=>c[0]==="identification_start").map(c=>c[2]),[false,false,true]);
  assert.equal(h.get("identStates.rsLs.phase"),"running");
});

test("Motor rejected finite Action completes with failure, not an Apply candidate",async()=>{
  const h=motorHarness();
  await h.run();
  h.complete({txn:1,actionId:0x1101,operation:"identification.rsLs",status:"ErrState",ok:false});
  assert.equal(h.get("identStates.rsLs.phase"),"failed");
  assert.equal(h.get("applyCandidate"),null);
  await h.apply();
  assert.equal(h.calls.some(c=>c[0]==="identification_apply"),false);
});

function encoderHarness(){
  const page=source("encoder/EncoderPage.svelte");
  const functions=["handleKey","phaseSearchAvailable","startPhaseSearch","handleActionCompleted"];
  const actions=[], errors=[];
  let phaseHandle={txn:7,actionId:0x1002},reject;
  const context=vm.createContext({
    connection:{commissioning:{phaseSearch:true}},
    phaseState:"idle",phaseMessage:"",pendingPhaseHandle:null,
    homingState:"idle",homingMessage:"",pendingHomingHandle:null,
    onError:(error)=>errors.push(String(error)),
    startPhaseSearchAction:async()=>{actions.push("phase_search_start");if(reject)throw reject;return phaseHandle;},
  });
  vm.runInContext(compile(functions.map(n=>extract(page,n)).join("\n")),context);
  const get=x=>vm.runInContext(x,context);
  return {actions,errors,get,start:()=>get("startPhaseSearch()"),
    complete:evt=>{context.completed=evt;return get("handleActionCompleted(completed)")},
    setReject:e=>reject=e};
}
test("Encoder Phase Search: start -> Running -> matching ACTION_COMPLETE -> Success",async()=>{
  const h=encoderHarness();
  await h.start();
  assert.deepEqual(h.actions,["phase_search_start"]);
  assert.equal(h.get("phaseState"),"running");
  h.complete({txn:7,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"success");
});
test("Encoder phase-search Action rejection and completion failure remain visible",async()=>{
  const h=encoderHarness();
  h.setReject(new Error("Motor already RUN"));
  await h.start();
  assert.equal(h.get("phaseState"),"failed");
  assert.match(h.get("phaseMessage"),/Motor already RUN/);
  assert.equal(h.actions.length,1);
});
