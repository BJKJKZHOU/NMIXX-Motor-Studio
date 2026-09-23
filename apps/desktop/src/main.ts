import "@vscode-elements/elements/dist/bundled.js";
import "@vscode/codicons/dist/codicon.css";
import "./theme.css";

import { mount } from "svelte";
import App from "./App.svelte";

mount(App, {
  target: document.getElementById("app")!,
});
