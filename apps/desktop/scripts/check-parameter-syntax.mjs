import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { fileURLToPath } from "node:url";
import ts from "typescript";

const root = fileURLToPath(new URL("../", import.meta.url));
const files = [
  "src/parameters/api.ts", "src/parameters/codec.ts", "src/parameters/drafts.ts",
  "src/parameters/state.ts", "src/parameters/editor.ts", "src/parameters/persistence.ts",
  "src/actions/api.ts", "src/motion/parameters.ts", "src/motion/store.ts",
  "src/App.svelte", "src/motor/MotorPage.svelte", "src/encoder/EncoderPage.svelte",
  "src/limits/LimitsPage.svelte", "src/control/ControlPage.svelte",
  "src/control/ControlTuningPage.svelte", "src/motion/MotionPage.svelte",
  "src/parameters/ParameterTablePage.svelte",
];
let errors = 0;
for (const file of files) {
  const text = readFileSync(resolve(root, file), "utf8");
  const source = file.endsWith(".svelte") ? text.match(/<script\b[^>]*>([\s\S]*?)<\/script>/)?.[1] : text;
  if (!source) throw new Error(`No script found: ${file}`);
  const result = ts.transpileModule(source, {
    fileName: `${file}.ts`, reportDiagnostics: true,
    compilerOptions: { target: ts.ScriptTarget.ES2022, module: ts.ModuleKind.ESNext, isolatedModules: true },
  });
  const diagnostics = (result.diagnostics ?? []).filter((d) => d.category === ts.DiagnosticCategory.Error);
  for (const diagnostic of diagnostics) {
    console.error(`${file}: ${ts.flattenDiagnosticMessageText(diagnostic.messageText, "\n")}`);
    errors++;
  }
}
console.log(`TypeScript script syntax: ${files.length} files, ${errors} errors (not a Svelte template/build check).`);
process.exitCode = errors ? 1 : 0;
