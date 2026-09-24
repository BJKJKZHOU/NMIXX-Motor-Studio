const assert = require("node:assert/strict");
const { spawnSync } = require("node:child_process");
const path = require("node:path");
const { test } = require("node:test");

function findPython() {
  const candidates = process.platform === "win32"
    ? [["py", ["-3"]], ["python", []], ["python3", []]]
    : [["python3", []], ["python", []]];

  for (const [program, prefix] of candidates) {
    const probe = spawnSync(program, [...prefix, "-c", "import sys; print(sys.version_info[0])"], {
      encoding: "utf8",
    });
    if (!probe.error && probe.status === 0 && probe.stdout.trim() === "3") {
      return { program, prefix };
    }
  }
  throw new Error("Python 3 is required to validate built-in Automation scripts");
}

test("all built-in Automation Python assets parse with Python 3", () => {
  const { program, prefix } = findPython();
  const root = path.resolve(__dirname, "../../../crates/nmixx-app/src/automation/assets");
  const files = [
    "nmixx.py",
    "runtime_diagnosis.py",
    "motion_workflow.py",
    "parameter_workflow.py",
  ].map((name) => path.join(root, name));

  const parser = [
    "import ast, pathlib, sys",
    "for filename in sys.argv[1:]:",
    "    source = pathlib.Path(filename).read_text(encoding='utf-8')",
    "    ast.parse(source, filename=filename)",
  ].join("\n");

  const result = spawnSync(program, [...prefix, "-c", parser, ...files], {
    encoding: "utf8",
  });
  assert.equal(
    result.status,
    0,
    `Python syntax validation failed:\n${result.stderr || result.stdout}`,
  );
});
