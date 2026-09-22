# Shared Parameter state: implementation and verification

This document records the implementation boundary of the shared-service rule in
`ARCHITECTURE.md` and the editor rules in `UI_INTERACTION_RULES.md`.

## Ownership

```text
Device RAM
    <-> ApplicationSession / ParameterService
            -> one committed cache (value OR read error)
                -> parameters/state.ts (read-only frontend projection)
                    -> Parameters / Motor / Encoder / Limits
                    -> Control Architecture / Control Tuning / Motion
```

The Parameters table is an expert view, not the owner or initializer of other pages.
A page declares its parameter symbols with `selectParameters` and uses
`createParameterEditor` for edits. It does not load its own committed cache, subscribe
to raw parameter events or provide a list of dependent readbacks.

`parameters/api.ts` is only the Tauri transport adapter. `parameters/state.ts` owns
one connection-scoped subscription and serialized cache mirroring. Page entry reads
the already-populated projection; it does not trigger a new device read.

Host-only Motion command state (Absolute/Incremental, incremental delta and Repeat),
Scope acquisition and waveform presentation remain separate domains. Layout, tab
selection and uncommitted text are local view state, not firmware parameter copies.

## Write and readback

A write validates and sends one firmware value, then rereads the written parameter
and its affected group in ParameterService. Firmware owns all calculations.

The current affected groups cover current-loop bandwidth/source/gains,
speed-loop bandwidth/source/gains, model-dependent control and limit values,
operating limits, encoder/reference configuration and motor-mode targets.
Only parameters advertised as readable by the loaded schema are included.
Adding a firmware dependency requires updating this shared table, not each page.

Device read groups and write/readback groups are serialized. Cache changes are
published as a group after the reads finish. Identical values do not generate a
change event. Failed reads replace stale cache values with explicit errors; recovery
is also a change. The write API distinguishes a rejected write from a successful RAM
write whose readback failed.

Initial Read and explicit global Read populate the same cache. ApplicationSession
refreshes parameters after immediate configuration Actions and before forwarding
finite Action completion events. Readback continues independently of page lifetime.
The Tauri bridge only forwards Application results and events.

AxDr's Save and Identification Apply responses represent completed synchronous
execution. Do not wait for an additional ACTION_COMPLETE for these two operations.

## Editing

`ParameterDrafts` contains only uncommitted text. It never stores a committed value.
The shared editor supplies the latest committed display text when no draft exists.

- Typing changes a draft only.
- Enter captures that draft before blur, submits the shared write API, and displays
  the canonical readback.
- Escape or uncommitted blur discards the draft and reveals the latest committed value.
- Telemetry updates and same-parameter external changes do not overwrite an active draft.
- A connection change clears old drafts and invalidates pending frontend responses.
- Selectors commit one complete value immediately; failed writes do not become
  committed display state.

Control Tuning's Copy Accel to Decel captures the command text before either write.
The two writes are sequential, not a claimed firmware transaction; a failure is
reported and successful partial readbacks remain visible.

## Persistence presentation

The frontend baseline is presentation state, not a host implementation of Flash.
It is initialized from the first successful connection snapshot. Existing unsaved
changes made before that connection cannot be inferred without a firmware persisted
snapshot. Within the connection, all views compare the same canonical values against
that baseline.

Save is serialized with GUI writes. Only a successful firmware Save followed by
successful synchronization establishes a new presentation baseline. Failed Save or
failed synchronization does not clear modified indications. Firmware still decides
which fields are persistent.

## Motion preview

Preview uses one ParameterService cache snapshot and the host Motion command model.
It performs no device reads and emits no parameter events. Its error belongs to the
preview panel and cannot prevent already-known parameters from being displayed.
Frontend preview requests are restricted to input changes and coalesced while an
IPC request is active. Actual Run/tuning preparation retains its execution-time
reads; display rendering is not a substitute for those reads.

## Automated checks

From `apps/desktop` with the existing dependencies installed:

```sh
npm run test:parameters
npm run check:parameter-syntax
npm run build
```

`test:parameters` compiles the pure parameter codec/draft/Motion conversion modules
with TypeScript, executes their Node tests, and checks source ownership boundaries.
It does not pretend to be a WebView integration test. `check:parameter-syntax`
checks TypeScript and component script syntax only; the Vite/Svelte build is still
required to compile templates. No new runtime or test-framework dependency is added.

From the repository root:

```sh
cargo test --workspace
```

ParameterService includes regression tests for unchanged reads, invalidation and
recovery, affected groups and batched notifications. Motion retains its conversion
and host-config tests and adds a speed-preview limit test.

## Device/UI acceptance

Connect a device and visit every Parameter-backed page without opening Parameters
first. Edit current bandwidth and a direct gain from the generic table, architecture
and tuning views in turn; confirm source and all dependent gains agree. Repeat with
motor-model values while firmware is in Bandwidth and Manual ownership modes.

Type slowly while status telemetry and bus voltage update. Verify the draft remains,
then verify Enter, Escape, blur and an intentionally rejected write. Repeat across
connection changes. Leave Motion open without input and verify preview does not
create a self-sustaining parameter-read loop.

Complete identification Apply, phase search and zero-setting while switching pages,
then inspect the shared results. Test successful and rejected Save and a simulated
readback failure. These checks require the actual firmware/WebView and are separate
from source-only tests; running the test suite does not actuate a motor.
