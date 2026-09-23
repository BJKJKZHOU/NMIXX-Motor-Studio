# NMIXX Motor Studio

NMIXX Motor Studio is a Rust-first motor-control host runtime and tooling stack.

The long-term product boundary is:

```text
GUI / CLI / Automation
        |
        v
Application API
        |
        v
Rust Application Core
        |
        v
Rust Device Core
        |
        v
Protocol / Transport
        |
        v
Motor Controller
```

The GUI is a client, not the product core. Automation is intentionally language-agnostic.

## Workspace

- `nmixx-core`: wire format, transport-independent protocol, device primitives.
- `nmixx-app`: application semantics and future public Application API.
- `nmixx-cli`: CLI client / early API validation surface.
- `apps/desktop`: Svelte frontend and Tauri desktop client.

## Desktop development

Install Node.js 22.12 or newer in the 22.x series, npm, a stable Rust toolchain,
and the [Tauri 2 system prerequisites](https://v2.tauri.app/start/prerequisites/)
for your operating system. Linux needs the GTK/WebKit development libraries.
From the repository root:

```sh
cd apps/desktop
npm ci
npm run tauri -- dev
```

This is the normal desktop development entry point. Tauri starts Vite on
`127.0.0.1:1420` through `beforeDevCommand` and then starts the Rust desktop process.
Keep this command running and stop it with Ctrl+C. Do not start a competing Vite
process on the same port.

The development executable in `target/debug` uses the configured Vite dev URL;
launching that executable alone is not a self-contained desktop launch. A device
and HostSchema are needed to connect to firmware, not to display the workbench.

Bare `cargo run` at the workspace root is ambiguous because the workspace contains
multiple binaries. To run the CLI explicitly:

```sh
cargo run -p nmixx-cli --bin nmixxctl -- --help
```

## Checks and production build

From `apps/desktop`:

```sh
npm run test:parameters
npm run test:plots
npm run build
```

`build` first runs the official `svelte-check` over the full frontend, including
Svelte templates, then runs Vite. Type errors and missing template names fail the
command. The checker is pinned to 4.7.6 and invoked through `npm exec`; its first
execution needs npm registry access or a populated npm tool cache. This tooling
invocation does not modify the application's package manifest or lockfile.
`npm run check` runs the same check without producing a bundle.

`check:parameter-syntax` and the focused Node tests are not replacements for that
check or a real-browser startup test. The plot tests execute component scripts
with mocked transport/chart lifecycles; they do not mount a WebView.

From the repository root, also run:

```sh
cargo test --workspace
```

To build the desktop executable with embedded frontend assets:

```sh
cd apps/desktop
npm run tauri -- build --no-bundle
```

On Linux the workspace executable is
`target/release/nmixx-motor-studio-desktop`. Unlike the development executable,
this build does not require a running Vite server. It still requires the platform's
runtime libraries. `npm run build` alone only creates frontend assets in `dist`.

`src-tauri/tauri.conf.json` currently sets `bundle.active` to `false`; installer
packaging is not configured. The command above builds an executable, not an
AppImage, Debian/RPM package, or installer. Packaging is separate from restoring
application startup.

## Disconnected browser startup regression

The optional smoke test uses Playwright with the real Vite/Svelte frontend and
the official Tauri IPC mocks. It rejects motor commands and never opens a device.
It checks the initial Connection workbench, navigation, Scope rendering and a
standalone two-axis Motion preview under `C`, `POSIX`, `en-US` and `zh-CN` language
values. A missing browser or dependency is a failure, not a skipped success.

From `apps/desktop`, in a Python virtual environment:

```sh
python -m pip install -r scripts/requirements-startup.txt
python -m playwright install chromium webkit
npm run test:startup
npm run test:startup -- --browser webkit
```

Playwright's browser system dependencies must also be installed. The runner starts
its own Vite instance on port 1431 (`--port` can change it), and stops it afterward.
An installed Chromium can be selected with `--executable /path/to/chromium`.
The fixture is `tests/startup.html`, outside Vite's production entry point. Locale
and IPC overrides exist only in the test; production does not patch Navigator or
Intl. This verifies frontend startup, not native Tauri integration or motor behavior.

## Phase 1

The first phase targets the current AxDr_L firmware protocol without importing its temporary host scripts:

1. canonical CAN FD frame model;
2. USB CDC `AXDR` envelope;
3. Parameter request/response primitives;
4. Action accepted/completed semantics;
5. then serial transport and a minimal CLI.

The current firmware remains the protocol authority. `tools/*.py` in the firmware repository are treated as disposable test scripts, not as the host architecture source.
