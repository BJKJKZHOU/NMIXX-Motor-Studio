// Real shared Parameter editor + Events command callbacks under fake IPC.
// Covers Parameter table, Limits and Control Architecture shared write behavior.
const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");

const root = join(__dirname, "../src");
const source = name => readFileSync(join(root,name), "utf8");
const compile = code => ts.transpileModule(code, {
  compilerOptions: {target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}
}).outputText;
function moduleFrom(sourceText, dependencies = {}) {
  const exports = {};
  vm.runInNewContext(compile(sourceText), {
    exports,
    require(name) {
      if (!(name in dependencies)) throw new Error("Missing test dependency "+name);
      return dependencies[name];
    },
    console,
  });
  return exports;
}
function store(initial, start) {
  let current = initial, stop;
  const subscribers = new Set();
  return {
    subscribe(callback) {
      subscribers.add(callback);
      if (!stop && start) stop = start();
      callback(current);
      return () => { subscribers.delete(callback); if (!subscribers.size) {stop?.();stop=undefined;} };
    },
    set(value) {current=value;for (const callback of subscribers) callback(value);},
    get() {return current;},
  };
}
function editorHarness({ access = "rw", typeName = "f32", currentValue = 1.5, saving = false } = {}) {
  const name = "PARAM_CTRL_SPEED_KP";
  const meta = {symbol:name,id:0x0a11,label:"Manual Speed Kp",typeName,access,
    allowedSymbols:[],allowed:[],description:"",persistent:true,default:null,unit:null};
  let current={type:typeName,value:currentValue};
  const view=store({
    epoch:1,loading:false,saving,metadata:{[name]:meta},
    values:{[name]:current},writing:new Set(),
  });
  const writes=[], errors=[];
  let writeFailure;
  const drafts = moduleFrom(source("parameters/drafts.ts"));
  const codec = moduleFrom(source("parameters/codec.ts"));
  const modules = {
    "svelte/store":{get:s=>s.get(),writable:store},
    "./drafts":drafts, "./codec":codec,
    "./state":{commitParameter:async(id,value)=>{
      writes.push({id,value});
      if(writeFailure)throw writeFailure;
      current={type:value.type,value:value.value};
      view.set({...view.get(),values:{[name]:current}});
      return{id,value};
    }},
  };
  const editor = moduleFrom(source("parameters/editor.ts"), modules)
    .createParameterEditor(view);
  let latest;
  const stop=editor.subscribe(value=>latest=value);
  function keydown(key){
    let prevented=false,blurred=false;
    editor.keydown({key,preventDefault(){prevented=true;},currentTarget:{blur(){blurred=true;}}},name,
      error=>errors.push(String(error)));
    return {prevented,blurred};
  }
  return {name,meta,editor,writes,errors,view,keydown,latest:()=>latest,
    fail:error=>writeFailure=error,close:stop};
}
const tick=()=>new Promise(resolve=>setImmediate(resolve));

test("Parameter / Control: Enter sends exactly one typed Parameter WRITE and adopts readback",async()=>{
  const h=editorHarness();
  h.editor.edit(h.name,"2.75");
  assert.equal(h.latest().dirty.has(h.name),true);
  const event=h.keydown("Enter");
  assert.equal(event.prevented,true);
  assert.equal(event.blurred,true);
  await tick();
  assert.equal(h.writes.length,1);
  assert.equal(h.writes[0].id,0x0a11);
  assert.equal(h.writes[0].value.type,"f32");
  assert.equal(h.writes[0].value.value,2.75);
  assert.equal(h.latest().drafts[h.name],"2.75");
  assert.equal(h.latest().dirty.has(h.name),false);
  h.close();
});

test("Parameter / Limits: Escape discards draft without sending any device command",async()=>{
  const h=editorHarness();
  h.editor.edit(h.name,"9000");
  assert.equal(h.latest().dirty.has(h.name),true);
  h.keydown("Escape");
  await tick();
  assert.equal(h.writes.length,0);
  assert.equal(h.latest().dirty.has(h.name),false);
  assert.equal(h.latest().drafts[h.name],"1.5");
  h.close();
});

test("Parameter: device WRITE rejection reports error and reverts to last committed value",async()=>{
  const h=editorHarness();
  h.fail(new Error("ERR_RANGE"));
  h.editor.edit(h.name,"99999");
  h.keydown("Enter");
  await tick();
  assert.equal(h.writes.length,1);
  assert.match(h.errors[0],/ERR_RANGE/);
  assert.equal(h.latest().drafts[h.name],"1.5");
  h.close();
});

test("Limits: readonly effective or hardware value never dispatches WRITE",async()=>{
  const h=editorHarness({access:"ro"});
  h.editor.edit(h.name,"3.4");
  h.keydown("Enter");
  await tick();
  assert.equal(h.writes.length,0);
  assert.match(h.errors[0],/read-only/);
  h.close();
});

test("Parameter: Save busy state blocks writes and shows a recoverable error",async()=>{
  const h=editorHarness({saving:true});
  h.editor.edit(h.name,"3.4");
  h.keydown("Enter");
  await tick();
  assert.equal(h.writes.length,0);
  assert.match(h.errors[0],/already in progress/);
  h.close();
});

function extract(sourceText,name){
  const regex=new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*`, "m");
  const beginning=sourceText.match(regex);
  assert.ok(beginning, name+" handler exists");
  if(beginning[0].trimEnd().endsWith("}"))return beginning[0];
  const full=sourceText.match(new RegExp(`^  (?:async )?function ${name}\\([^\\n]*?\\)[^\\n]*\\{[\\s\\S]*?^  \\}`, "m"));
  assert.ok(full, name+" complete handler exists");
  return full[0];
}

test("Events: Clear Fault sends Protection Clear before Recheck, does not clear history",async()=>{
  const page=source("events/EventsPage.svelte");
  const calls=[], errors=[];
  const ctx=vm.createContext({
    busy:false,connection:{},calls,
    clearProtection:async()=>calls.push("ACTION_PROTECTION_CLEAR"),
    recheckProblems:async()=>calls.push("READ_PROTECTION_AND_ENCODER"),
    clearProblemHistory:async()=>calls.push("CLEAR_HISTORY"),
    onError:error=>errors.push(String(error)),
  });
  vm.runInContext(compile(["perform","clearFault"].map(n=>extract(page,n)).join("\n")),ctx);
  await vm.runInContext("perform(clearFault)",ctx);
  assert.equal(ctx.busy,false);
  assert.deepEqual(calls,["ACTION_PROTECTION_CLEAR","READ_PROTECTION_AND_ENCODER"]);
  assert.equal(errors.length,0);
});

test("Events: failed Protection Clear does not claim Recheck or clear active faults",async()=>{
  const page=source("events/EventsPage.svelte");
  const calls=[],errors=[];
  const ctx=vm.createContext({busy:false,connection:{},
    clearProtection:async()=>{calls.push("ACTION_PROTECTION_CLEAR");throw new Error("ERR_STATE");},
    recheckProblems:async()=>calls.push("READ_RECHECK"),
    onError:error=>errors.push(String(error))});
  vm.runInContext(compile(["perform","clearFault"].map(n=>extract(page,n)).join("\n")),ctx);
  await vm.runInContext("perform(clearFault)",ctx);
  assert.deepEqual(calls,["ACTION_PROTECTION_CLEAR"]);
  assert.match(errors[0],/ERR_STATE/);
  assert.equal(ctx.busy,false);
});
