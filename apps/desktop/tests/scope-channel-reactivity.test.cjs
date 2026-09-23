const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const { before, test } = require("node:test");

let code;
before(async () => {
  const { compile } = await import("svelte/compiler");
  const filename = path.resolve(__dirname, "../src/analysis/scope/ScopeEchartsPage.svelte");
  code = compile(fs.readFileSync(filename, "utf8"), {
    filename,
    generate: "client",
    dev: true,
  }).js.code;
});

// Callback-only tests do not observe compiler-inserted untrack() around a
// zero-argument template helper. Inspect the real page's generated bindings.
// This is a compiler regression check, not a WebView/device integration test.
test("Scope visible channels track the registry and checkbox selection", () => {
  assert.match(code, /legacy_pre_effect\(\s*\(\)\s*=>\s*\(\$\.get\(channels\),\s*\$\.get\(selectedIds\)\)/);
  assert.match(code, /\$\.set\(visibleChannels,\s*\$\.get\(channels\)\.filter\(/);
  assert.doesNotMatch(code, /untrack\(activeChannels\)/);
});

test("Scope chart receives the reactive channel list instead of a frozen helper result", () => {
  assert.match(code, /get channels\(\)\s*\{\s*return \$\.get\(visibleChannels\);/);
});
