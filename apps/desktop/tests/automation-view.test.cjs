// Actual state.ts functions with injected stores and IPC. These are not Svelte
// compiler, Tauri/WebView or motor tests.
const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const vm = require('node:vm');
const { test } = require('node:test');
const ts = require('typescript');
function store(value) { return { _get:()=>value, set(v){value=v}, update(fn){value=fn(value)} }; }
function task(id,state='COMPLETED',effects=[]) { return {taskId:id,state,name:'script.py',logs:[],effects,firstSequence:1,nextSequence:1,exitCode:0,message:null}; }
function setup() {
  const state=store({epoch:1,connected:true});
  const scope=store(undefined),saved=[],motions=[];
  let read=async()=>task(1),run=async()=>task(1),clock;
  const api={readTask:(...args)=>read(...args),runScript:(...args)=>run(...args),builtinScript:async()=>({name:'runtime_diagnosis.py',source:'print(1)'}),openScript:async()=>({name:'external.py',source:'print(2)'})};
  const modules={
    'svelte/store':{get:s=>s._get(),writable:store},
    '../refreshScheduler':{subscribeRefresh:(interval,fn)=>{assert.equal(interval,100);clock=fn;return()=>{clock=undefined}}},
    '../parameters/state':{parameterState:state,acceptSavedParameterBaseline:v=>saved.push(v)},
    '../motion/store':{acceptMotionSnapshot:v=>motions.push(v),waitMotionUpdates:async()=>{}},
    './operations':{scopeOperation:scope},'./api':api,
  };
  const filename=path.join(__dirname,'../src/automation/state.ts');
  const source=ts.transpileModule(fs.readFileSync(filename,'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS}}).outputText;
  const exports={};
  vm.runInNewContext(source,{exports,require:n=>{if(!(n in modules))throw Error(n);return modules[n]},console});
  return {s:exports,state,scope,saved,motions,setRead:fn=>read=fn,setRun:fn=>run=fn,tick:()=>clock?.()};
}
const pause=()=>new Promise(resolve=>setImmediate(resolve));
test('script completion mirrors canonical Scope/Motion without another device command',async()=>{
  const h=setup();await h.s.ensureDocument();
  const effects=[{sequence:1,kind:'scope.changed',payload:{config:{channels:[{id:17,rate:'normal'}],historySeconds:10},state:'STOPPED'}},
    {sequence:2,kind:'motion.changed',payload:{positionCommand:'absolute',repeat:false,incrementalDeltaTurn:1}}];
  h.setRun(async()=>task(1,'COMPLETED',effects));h.setRead(async()=>task(1,'COMPLETED',effects));
  await h.s.runSelected();
  assert.equal(h.scope._get().state,'STOPPED');assert.equal(h.motions.length,1);
  await h.s.refreshTask();assert.equal(h.motions.length,1);
});
test('Save uses captured baseline rather than a later current cache',async()=>{
  const h=setup();await h.s.ensureDocument();
  const baseline=[{id:17,value:{type:'f32',value:1},error:null}];
  const result=task(1,'COMPLETED',[{sequence:1,kind:'config.saved',payload:baseline}]);
  h.setRun(async()=>result);h.setRead(async()=>result);
  await h.s.runSelected();assert.deepEqual(h.saved,[baseline]);
});
test('old workflow effects are not applied to reconnected device',async()=>{
  const h=setup();await h.s.ensureDocument();
  h.setRun(async()=>task(1,'RUNNING'));h.setRead(async()=>task(1,'RUNNING'));
  await h.s.runSelected();h.s.clearAutomationProjection();h.state.set({epoch:2,connected:true});
  h.setRead(async()=>task(1,'COMPLETED',[{sequence:1,kind:'config.saved',payload:[1]}]));
  await h.s.refreshTask();assert.equal(h.saved.length,0);
});
test('application-owned task continues to be observed without Automation page',async()=>{
  const h=setup();h.s.taskState.set(task(1,'RUNNING'));
  const stop=h.s.startAutomationTracking(()=>true);
  h.setRead(async()=>task(1,'COMPLETED'));h.tick();await pause();await pause();
  assert.equal(h.s.taskState._get().state,'COMPLETED');stop();
});
test('a new task refresh is coalesced behind an older pending IPC response',async()=>{
  const h=setup();await h.s.ensureDocument();let release;
  h.setRead(()=>new Promise(resolve=>{release=resolve}));
  const pending=h.s.refreshTask();await pause();
  h.setRun(async()=>task(2,'COMPLETED'));await h.s.runSelected();
  h.setRead(async()=>task(2,'COMPLETED'));release(task(1,'RUNNING'));await pending;await pause();await pause();
  assert.equal(h.s.taskState._get().taskId,2);assert.equal(h.s.taskState._get().state,'COMPLETED');
});
