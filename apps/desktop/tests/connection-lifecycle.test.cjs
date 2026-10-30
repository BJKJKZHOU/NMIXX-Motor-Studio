// Isolated component command tests, not a browser/WebView mount test.
const { test } = require('node:test');
const assert = require('node:assert/strict');
const { readFileSync } = require('node:fs');
const { join } = require('node:path');
const vm = require('node:vm');
const ts = require('typescript');

function harness(operation) {
  const source = readFileSync(join(__dirname, '../src/connection/ConnectionPage.svelte'), 'utf8');
  const match = source.match(/  async function disconnect\(\) \{[\s\S]*?\n  \}/);
  assert.ok(match, 'disconnect command exists');
  const errors = [];
  let disconnected = 0;
  let calls = 0;
  const context = vm.createContext({
    disconnectDevice: () => { calls++; return operation(); },
    onDisconnected: () => { disconnected++; },
    onError: (error) => errors.push(String(error)),
  });
  vm.runInContext('let busy = false;\n' + match[0], context);
  return {
    disconnect: () => vm.runInContext('disconnect()', context),
    busy: () => vm.runInContext('busy', context),
    disconnected: () => disconnected,
    calls: () => calls,
    errors,
  };
}

test('successful backend close clears the view once', async () => {
  const h = harness(() => Promise.resolve());
  await h.disconnect();
  assert.equal(h.disconnected(), 1);
  assert.equal(h.busy(), false);
  assert.deepEqual(h.errors, []);
});
test('failed backend close keeps the connected view and its safety controls', async () => {
  const h = harness(() => Promise.reject(new Error('motor is still RUN')));
  await h.disconnect();
  assert.equal(h.disconnected(), 0);
  assert.equal(h.busy(), false);
  assert.deepEqual(h.errors, ['Error: motor is still RUN']);
});
test('an in-flight disconnect cannot be submitted twice', async () => {
  let resolve;
  const operation = new Promise((done) => { resolve = done; });
  const h = harness(() => operation);
  const pending = h.disconnect();
  assert.equal(h.busy(), true);
  await h.disconnect();
  assert.equal(h.calls(), 1);
  assert.equal(h.disconnected(), 0);
  resolve();
  await pending;
  assert.equal(h.disconnected(), 1);
  assert.equal(h.busy(), false);
});

test('Connect rejects duplicate requests while the first device initialization is pending', async () => {
  const page = readFileSync(join(__dirname, '../src/connection/ConnectionPage.svelte'), 'utf8');
  const handler = page.match(/  async function connect\(\) \{[\s\S]*?\n  \}/)?.[0];
  assert.ok(handler);
  let finish, calls = 0, published = 0;
  const ctx = vm.createContext({
    busy:false,connection:undefined,port:'COM7',schemaPath:'axdr-host-schema.toml',
    onSchemaPathChanged:()=>{},onConnected:()=>published++,onError:()=>{},
    connectDevice:()=>{calls++;return new Promise(resolve=>{finish=resolve;});},
  });
  vm.runInContext(handler,ctx);
  const first=vm.runInContext('connect()',ctx);
  await vm.runInContext('connect()',ctx);
  assert.equal(calls,1);
  finish({port:'COM7'});
  await first;
  assert.equal(published,1);
  assert.equal(vm.runInContext('busy',ctx),false);
});

test('late port discovery does not overwrite a manually changed endpoint', async () => {
  const page = readFileSync(join(__dirname, '../src/connection/ConnectionPage.svelte'), 'utf8');
  const fn=name=>page.match(new RegExp('  (?:async )?function '+name+'\\([\\s\\S]*?\\n  \\}', 'm'))?.[0];
  assert.ok(fn('preferredPort'));
  assert.ok(fn('refreshPorts'));
  let complete;
  const ctx=vm.createContext({
    ports:[],port:'COM7',connection:undefined,busy:false,onError:()=>{},
    listDevices:()=>new Promise(resolve=>complete=resolve),
  });
  const code=ts.transpileModule(fn('preferredPort')+'\n'+fn('refreshPorts'),{
    compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.None},
  }).outputText;
  vm.runInContext(code,ctx);
  const refresh=vm.runInContext('refreshPorts()',ctx);
  ctx.port='COM9';
  complete(['COM2','COM7','COM9']);
  await refresh;
  assert.equal(ctx.port,'COM9');
  assert.equal(ctx.ports.length,3);
});
