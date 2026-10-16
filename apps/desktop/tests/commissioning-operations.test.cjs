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
    $uiLanguage:"zh-CN",
    translateFormat:(key, language, values)=>{
      assert.equal(key,"motor.identification.enable.confirm");
      assert.equal(language,"zh-CN");
      return values.kind+" 辨识需要使能电机。\\n\\n是否使能并继续？";
    },
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
  assert.match(h.calls.find(c=>c[0]==="confirm")[1],/辨识需要使能电机/);
  assert.equal(h.calls.filter(c=>c[0]==="identification_start").length,1);
  h.setConfirm(true);
  await h.run();
  assert.deepEqual(h.calls.filter(c=>c[0]==="identification_start").map(c=>c[2]),[false,false,true]);
  assert.equal(h.get("identStates.rsLs.phase"),"running");
});

test("Motor ignores stale same-operation completions with a different transaction",async()=>{
  const h=motorHarness();
  await h.run();
  h.complete({txn:9,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true});
  assert.equal(h.get("identStates.rsLs.phase"),"running");
  assert.equal(h.get("applyCandidate"),null);
  h.complete({txn:1,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true});
  assert.equal(h.get("identStates.rsLs.phase"),"ready");
});

test("Motor preserves a matching completion received before the start response",async()=>{
  const h=motorHarness();
  let resolveStart;
  h.setResponse(()=>new Promise(resolve=>{resolveStart=resolve;}));
  const start=h.run();
  h.complete({txn:1,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true});
  assert.equal(h.get("identStates.rsLs.phase"),"idle");
  resolveStart({status:"started",handle:{txn:1,actionId:0x1101}});
  await start;
  assert.equal(h.get("identStates.rsLs.phase"),"ready");
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
  let phaseHandle={txn:7,actionId:0x1002},reject,phaseResponder=()=>phaseHandle;
  const context=vm.createContext({
    connection:{commissioning:{phaseSearch:true}},
    phaseState:"idle",phaseMessage:"",pendingPhaseHandle:null,earlyPhaseCompletion:undefined,phaseRequestGeneration:0,
    homingState:"idle",homingMessage:"",pendingHomingHandle:null,earlyHomingCompletion:undefined,homingRequestGeneration:0,
    onError:(error)=>errors.push(String(error)),
    startPhaseSearchAction:async()=>{actions.push("phase_search_start");if(reject)throw reject;return phaseResponder();},
  });
  vm.runInContext(compile(functions.map(n=>extract(page,n)).join("\n")),context);
  const get=x=>vm.runInContext(x,context);
  return {actions,errors,get,start:()=>get("startPhaseSearch()"),
    complete:evt=>{context.completed=evt;return get("handleActionCompleted(completed)")},
    setReject:e=>reject=e,setPhaseResponder:fn=>phaseResponder=fn};
}
test("Encoder Phase Search: start -> Running -> matching ACTION_COMPLETE -> Success",async()=>{
  const h=encoderHarness();
  await h.start();
  assert.deepEqual(h.actions,["phase_search_start"]);
  assert.equal(h.get("phaseState"),"running");
  h.complete({txn:7,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"success");
});
test("Encoder ignores stale same-operation completion from a different transaction",async()=>{
  const h=encoderHarness();
  await h.start();
  h.complete({txn:8,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"running");
  h.complete({txn:7,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"success");
});
test("Encoder correlates a completion that arrives before the start response",async()=>{
  const h=encoderHarness();
  let resolveStart;
  h.setPhaseResponder(()=>new Promise(resolve=>{resolveStart=resolve;}));
  const start=h.start();
  h.complete({txn:17,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"running");
  resolveStart({txn:7,actionId:0x1002});
  await start;
  assert.equal(h.get("phaseState"),"running");
  h.complete({txn:7,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true});
  assert.equal(h.get("phaseState"),"success");
});
test("Encoder ignores a start reply after the operation was cancelled",async()=>{
  const h=encoderHarness();
  let resolveStart;
  h.setPhaseResponder(()=>new Promise(resolve=>{resolveStart=resolve;}));
  const start=h.start();
  h.get('++phaseRequestGeneration; phaseState="stopped"; pendingPhaseHandle=null; earlyPhaseCompletion=undefined');
  resolveStart({txn:7,actionId:0x1002});
  await start;
  assert.equal(h.get("phaseState"),"stopped");
  assert.equal(h.get("pendingPhaseHandle"),null);
});
test("Encoder phase-search Action rejection and completion failure remain visible",async()=>{
  const h=encoderHarness();
  h.setReject(new Error("Motor already RUN"));
  await h.start();
  assert.equal(h.get("phaseState"),"failed");
  assert.match(h.get("phaseMessage"),/Motor already RUN/);
  assert.equal(h.actions.length,1);
});

test("Motor treats successful firmware completion with failed Parameter readback as unverified",async()=>{
  const h=motorHarness();
  await h.run();
  h.complete({txn:1,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true,readbackError:"cached parameter failed"});
  assert.equal(h.get("identStates.rsLs.phase"),"unverified");
  assert.equal(h.get("applyCandidate"),null);
  await h.apply();
  assert.equal(h.calls.some(c=>c[0]==="identification_apply"),false);
});

test("Encoder does not claim calibration success when Action readback failed",async()=>{
  const h=encoderHarness();
  await h.start();
  h.complete({txn:7,actionId:0x1002,operation:"encoder.phaseSearch",status:"Ok",ok:true,readbackError:"parameter readback lost"});
  assert.equal(h.get("phaseState"),"unverified");
  assert.match(h.get("phaseMessage"),/readback lost/);
});

test("parameter synchronization rejects cached read errors and recovers after a fresh valid snapshot",async()=>{
  const state={connected:true,entries:{
    1:{id:1,value:{type:"u8",value:1},error:null},
    2:{id:2,value:null,error:"ERR_IO"},
  }};
  const sourceText=source("parameters/state.ts");
  const fragment=sourceText.match(/^export async function synchronizeParameters\(\): Promise<void> \{[\s\S]*?^\}/m)?.[0];
  assert.ok(fragment);
  const ctx=vm.createContext({
    exports:{},epoch:10,mutable:{},get:()=>state,
    readableIds:()=>[1,2],sync:async()=>{},
    requireSession:()=>{if(!state.connected)throw Error("Device connection changed.");},
    report:()=>{},
  });
  vm.runInContext(compile(fragment),ctx);
  await assert.rejects(ctx.exports.synchronizeParameters(),/Parameter synchronization failed for 0x0002: ERR_IO/);
  state.entries[2]={id:2,value:{type:"u8",value:2},error:null};
  await ctx.exports.synchronizeParameters();
  state.connected=false;
  await assert.rejects(ctx.exports.synchronizeParameters(),/Device connection changed/);
});

test("Action completion bridge forwards firmware result with readback error and fences stale epochs",async()=>{
  let listener,disposed=false;
  const state={epoch:5};
  let rejectSync=false, holdSync=false, releaseSync;
  const received=[];
  const ctx=vm.createContext({
    exports:{},
    require(name){
      if(name==="@tauri-apps/api/core")return {invoke:async()=>({})};
      if(name==="@tauri-apps/api/event")return {listen:async(_,fn)=>{
        listener=fn;return()=>{disposed=true;};
      }};
      if(name==="svelte/store")return {get:()=>state};
      if(name==="../parameters/state")return {parameterState:state,synchronizeParameters:()=>{
        if(holdSync)return new Promise(resolve=>releaseSync=resolve);
        return rejectSync?Promise.reject(Error("readback failed")):Promise.resolve();
      }};
      throw Error(name);
    },
  });
  vm.runInContext(compile(source("actions/api.ts")),ctx);
  const unsubscribe=await ctx.exports.onActionCompleted(event=>received.push(event));
  const completion={txn:3,actionId:0x1101,operation:"identification.rsLs",status:"Ok",ok:true};
  listener({payload:completion});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(received.length,1);
  assert.equal(received[0].readbackError,undefined);
  rejectSync=true;
  listener({payload:completion});
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(received.length,2);
  assert.equal(received[1].ok,true);
  assert.match(received[1].readbackError,/readback failed/);
  rejectSync=false;holdSync=true;
  listener({payload:completion});
  state.epoch=6;releaseSync();
  await new Promise(resolve=>setImmediate(resolve));
  assert.equal(received.length,2);
  unsubscribe();
  assert.equal(disposed,true);
});

test("accepted Motor Stop with failed Parameter refresh reports ambiguous UI state, not rejected command",async()=>{
  const calls=[];
  const ctx=vm.createContext({
    exports:{},
    require(name){
      if(name==="@tauri-apps/api/core")return {invoke:async(name)=>{calls.push(name);return {txn:1,actionId:0x1003};}};
      if(name==="@tauri-apps/api/event")return {listen:async()=>()=>{}};
      if(name==="svelte/store")return {get:state=>state};
      if(name==="../parameters/state")return {parameterState:{epoch:1},synchronizeParameters:async()=>{throw new Error("cable unavailable");}};
      throw new Error(name);
    },
  });
  vm.runInContext(compile(source("actions/api.ts")),ctx);
  await assert.rejects(ctx.exports.stopMotor(),/Device accepted the command, but Parameter readback failed/);
  assert.deepEqual(calls,["motor_stop"]);
});
