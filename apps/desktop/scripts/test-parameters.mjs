import { mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { spawnSync } from "node:child_process";

const root = fileURLToPath(new URL("../", import.meta.url));
const output = mkdtempSync(join(tmpdir(), "nmixx-parameter-tests-"));
let status = 1;
try {
  writeFileSync(join(output, "package.json"), '{"type":"commonjs"}\n');
  const compiler = spawnSync(process.platform === "win32" ? "tsc.cmd" : "tsc", [
    "--strict", "--target", "ES2022", "--module", "commonjs", "--skipLibCheck",
    "--rootDir", "src", "--outDir", output,
    "src/parameters/types.ts", "src/parameters/codec.ts", "src/parameters/drafts.ts",
    "src/motion/types.ts", "src/motion/parameters.ts",
  ], { cwd: root, stdio: "inherit", shell: process.platform === "win32" });
  if (compiler.error) throw compiler.error;
  if (compiler.status === 0) {
    const test = spawnSync(process.execPath, ["--test", "tests/parameters.test.cjs"], {
      cwd: root, stdio: "inherit", env: { ...process.env, NMIXX_PARAMETER_TEST_BUILD: output },
    });
    if (test.error) throw test.error;
    status = test.status ?? 1;
  } else {
    status = compiler.status ?? 1;
  }
} finally {
  rmSync(output, { recursive: true, force: true });
}
process.exitCode = status;
