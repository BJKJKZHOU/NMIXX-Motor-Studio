const { test } = require("node:test");
const assert = require("node:assert/strict");
const { readFileSync } = require("node:fs");
const { join } = require("node:path");
const vm = require("node:vm");
const ts = require("typescript");
const source = name => readFileSync(join(__dirname, "..", "src", name), "utf8");

function store(value) {
  const listeners=new Set();
  return {
    _get:()=>value,
    set(next){value=next;for(const fn of listeners)fn(next);},
    update(fn){this.set(fn(value));},
    subscribe(fn){listeners.add(fn);fn(value);return()=>listeners.delete(fn);},
  };
}
function setup() {
  const writes=[],opens=[];
  const api={
    builtinScript:async kind=>({name:kind+".py",source:"print('example')",path:null,workingDirectory:null}),
    openScript:async path=>{opens.push(path);return{name:path.split("/").at(-1),source:"print('disk')",path,workingDirectory:"/scripts"};},
    saveScript:async(path,source,overwrite)=>{writes.push({path,source,overwrite});return{name:path.split("/").at(-1),path,source,workingDirectory:"/scripts"};},
  };
  const derived=(stores,fn)=>({
    _get:()=>fn(stores.map(s=>s._get())),
    subscribe(cb){
      const update=()=>cb(fn(stores.map(s=>s._get())));
      const stops=stores.map(s=>s.subscribe(update));
      return()=>stops.forEach(stop=>stop());
    },
  });
  const exports={};
  const js=ts.transpileModule(source("automation/documents.ts"),{
    compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.CommonJS},
  }).outputText;
  vm.runInNewContext(js,{
    exports,
    require:name=>{
      if(name==="svelte/store")return{get:s=>s._get(),writable:store,derived};
      if(name==="./api")return api;
      throw Error(name);
    },
  });
  return {d:exports,writes,opens};
}

test("script tabs retain independent live edits and selection",()=>{
  const {d}=setup();
  const a=d.newScript(),b=d.newScript();
  d.editScript(a.id,"from nmixx import client\nclient.run()");
  assert.match(d.scriptTabs._get()[0].source,/client.run/);
  assert.doesNotMatch(d.scriptTabs._get()[1].source,/client.run/);
  assert(d.isDirty(d.scriptTabs._get()[0]));
  d.selectScript(a.id);
  assert.equal(d.activeScript._get().id,a.id);
  d.closeScript(a.id);
  assert.equal(d.activeScript._get().id,b.id);
});
test("examples are editable documents, Save As does not overwrite bundled files",async()=>{
  const {d,writes}=setup();
  const tab=await d.openExample("motion");
  d.editScript(tab.id,"from nmixx import client\nclient.stop()");
  await assert.rejects(d.saveCurrentScript(),/Choose a file path/);
  await d.saveCurrentScript("/scripts/motor.py");
  assert.equal(writes[0].overwrite,false);
  assert.match(writes[0].source,/client.stop/);
  d.editScript(tab.id,"print('next')");await d.saveCurrentScript();
  assert.equal(writes[1].overwrite,true);
});
test("reopening the same script keeps its unsaved edits",async()=>{
  const {d,opens}=setup();
  const tab=await d.openLocalScript("/scripts/test.py");
  d.editScript(tab.id,"print('edited')");
  assert.equal((await d.openLocalScript("/scripts/test.py")).id,tab.id);
  assert.equal(opens.length,1);
  assert.equal(d.activeScript._get().source,"print('edited')");
});
test("Run uses current editor text and no feature-specific top-level script launchers",()=>{
  const page=source("automation/AutomationPage.svelte");
  assert.match(page,/const source = \$activeScript\.source/);
  assert.match(page,/runSelected\(document\)/);
  assert.doesNotMatch(page,/selectBuiltin\("motion"\)/);
  assert.match(source("automation/ScriptEditor.svelte"),/onEdit\(tab\.id, value\)/);
});
