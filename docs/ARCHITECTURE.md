# NMIXX Motor Studio architecture

## Product boundary

NMIXX Motor Studio is centered on a headless Rust Application Runtime.

```text
GUI -----------+
CLI -----------+--> Application API --> Application Core --> Device Core --> MCU
Automation ----+
```

The Application API is the stable product surface. No automation language is privileged.

## Maintainability goals

The project is expected to grow from a CLI-first host tool into a long-lived motor-control engineering application. Maintainability is therefore a first-class requirement, not a later cleanup task.

The architecture follows these rules:

1. **Dependencies point inward.** Clients depend on Application API, Application depends on Device Core, Device Core depends on protocol/wire/transport abstractions. Lower layers never depend on GUI, CLI, scripting, or commissioning workflows.
2. **One owner for device I/O.** A device connection is owned by one runtime/session. GUI, CLI and automation never open competing transports for the same session.
3. **Wire details stay in the core.** CAN IDs, DLC rules, USB framing, transaction matching and payload layouts must not leak into Application API or clients.
4. **Transport and protocol remain independent.** USB CDC, native CAN FD and future transports carry the same protocol frame model through adapters.
5. **Application behavior is shared.** State checks, resource arbitration, protection-aware behavior, task lifecycle and error semantics belong in `nmixx-app`, not separately in each client.
6. **Long operations use one Task model.** Identification, recording, replay and future commissioning workflows expose common lifecycle/progress/cancellation semantics.
7. **Automation is composition, not core policy.** Repeat-N tests, quality thresholds and commissioning sequences are normally built by calling the Application API instead of being hard-coded into the device core.
8. **Stable data contracts are generated where practical.** Firmware Parameter/Action schema should generate host IDs and metadata rather than requiring duplicated hand-maintained tables.
9. **No hidden global state.** Session, transport, task ownership and subscriptions are explicit objects with clear lifetimes.
10. **Errors preserve layer context.** Transport, wire/protocol, device and application failures remain distinguishable so failures can be diagnosed without string parsing.

## Layer rules

### `nmixx-core`

Owns transport-independent device capabilities:

- canonical CAN FD frame representation;
- USB CDC AXDR envelope;
- transport interfaces and adapters;
- protocol transaction/response handling;
- Parameter primitives;
- Action accepted/completed semantics;
- Event decoding;
- telemetry stream decoding.

It must not depend on UI, CLI, scripting language, commissioning workflow, or a specific test procedure.

Suggested internal dependency direction:

```text
wire <- transport
  ^        |
  +--- protocol <- device
```

Protocol code may use the canonical frame model but must not know whether a frame arrived through USB CDC or native CAN FD.

### `nmixx-app`

Owns host application semantics:

- device sessions and connection ownership;
- task lifecycle;
- resource arbitration;
- scope/recorder services;
- protection-aware application behavior;
- stable public Application API.

The Application API should expose domain concepts such as `parameter.get`, `motor.enable`, `action.start`, `stream.subscribe` and `task.cancel`, not raw CAN IDs or payload bytes.

### clients

GUI, CLI and automation are peers. They must not implement device protocol logic or duplicate application policy.

A client may format values, render a progress bar, or compose a workflow, but it must not reimplement transaction matching, action completion handling, protection rules or transport framing.

## Desktop GUI functional domains

The desktop client is split by **functional domain and shared internal capability**, not by visual rectangles or one-file-per-widget rules.

```text
App shell
  |
  +-- Connection
  |     +-- Serial
  |     +-- CAN FD
  |     +-- future transports
  |
  +-- Parameters
  |     +-- full parameter table
  |     +-- read/write/search/filter
  |     +-- import/export
  |
  +-- Analysis
  |     +-- shared acquisition
  |     +-- Scope
  |     +-- FFT
  |     +-- Bode
  |     +-- capture/export
  |
  +-- Motor
  +-- Encoder
  +-- Limits / Safety
  +-- Control
  +-- Control Tuning
  +-- Events
```

`App.svelte` is the workbench shell. It owns navigation, global connection summary, global notifications and the status bar. Device and analysis behavior belongs in domain modules.

### UI reuse boundary

NMIXX owns product semantics, information architecture and presentation style. Generic interaction mechanisms should use mature libraries when they are already solved well.

```text
Application API / motor semantics / commissioning workflow
                         |
                    NMIXX-owned

table state / plot interaction / terminal / editor / docking
                         |
                 mature infrastructure

colors / spacing / typography / icons / density / page composition
                         |
                    NMIXX-owned
```

Rules:

- Do not introduce a large opinionated visual framework merely to obtain a widget. NMIXX keeps its VS Code-like engineering-tool presentation and theme tokens.
- Prefer headless or low-presentation libraries for complex interaction behavior.
- A domain page must not duplicate Application API state or create a second write path for a device concept.
- `App.svelte` remains a shell; domain behavior belongs to domain modules.
- Reuse the existing `parameters/api.ts`, analysis acquisition service and other domain APIs rather than invoking Tauri directly from arbitrary components.
- Do not hand-roll sorting, filtering, column state, terminal emulation, code editing or IDE docking once the approved infrastructure is applicable.

Approved now:

- **VSCode Elements + Codicons** for basic controls and icons.
- **uPlot** for Scope/FFT/Bode plotting mechanics, including scales, axes, cursor and plugins.
- **Split.js** for simple fixed split panes.
- **TanStack Table** for Parameters and Events table state, sorting and filtering.

Introduce only when the corresponding product need exists:

- **Dockview** for draggable/persisted IDE-style panel layouts; do not replace simple Split.js layouts preemptively.
- **xterm.js** for an Automation terminal frontend.
- **Monaco Editor** for script editing.

Avoid using Ant Design, Material UI, Bootstrap, dashboard templates or a second general-purpose chart framework as the product's visual foundation.

### Workflow-oriented navigation

The primary Activity Bar is ordered by the normal motor commissioning and control workflow, not by software implementation modules.

```text
Connection
    |
    v
Motor + identification tools
    |
    v
Encoder + phase search / homing / zero
    |
    v
Limits / Safety
    |
    v
Control architecture
    |
    v
Control Tuning
    |
    v
Analysis: Scope / FFT / Bode
```

The order communicates the normal engineering sequence without turning the application into a mandatory wizard. Experienced users may jump directly to any page.

- **Connection** establishes the Device Session and transport. It is the first page and is independent from Scope or other analysis tools.
- **Motor** owns the motor parameter view. Unknown parameters are obtained through identification tools attached to this page instead of a separate top-level Identification page.
- **Encoder** configures feedback type/interface and generic encoder parameters. Phase search current, homing and zero-setting tools belong here because they establish position/electrical alignment.
- **Limits / Safety** configures user operating limits and other software safety boundaries while keeping hardware protection semantics distinct.
- **Control** chooses the operating mode and control architecture: loop controllers, startup strategy, observer and related defaults. It does not own the repeated motion-test tuning workflow.
- **Control Tuning** runs bounded tuning experiments: edit controller design parameters, configure one motion command, capture the synchronized response, and preserve the completed waveform for comparison.
- **Analysis** provides general-purpose continuous or manually triggered engineering analysis. Scope, FFT and Bode share acquisition and plotting infrastructure but are not responsible for the Control Tuning experiment workflow.

Cross-workflow expert tools are visually separated from the commissioning sequence. The generic **Parameters** table, **Events**, and future **Automation** entry belong to this secondary group rather than interrupting the primary workflow.

### Parameter is a shared service, not one page

The Parameter subsystem is the single source of truth for host-visible parameters. The generic Parameters page is an expert view over the complete registry, but domain pages reuse the same Parameter API:

```text
Parameter service
      |
      +--> Parameters table   (all parameters, direct read/write/import/export)
      +--> Motor page         (motor-related parameter view)
      +--> Encoder page       (feedback-related parameter view)
      +--> Limits page        (operating/safety limits)
      +--> Control page       (algorithm selection and defaults)
      +--> Control Tuning     (controller design parameters and active gains)
      +--> other domain pages
```

A value such as motor resistance must not have separate storage or write paths in the generic table and the Motor page. Both views call the same Application API entry.

The Parameters page uses TanStack Table for table behavior. TanStack owns sorting/filtering/table state; NMIXX owns markup, styling, device reads/writes and value editing semantics. Table code must not bypass `ParameterService`.

### Control setup and tuning are different workflows

The **Control** page selects the active control structure and supplies usable defaults. The **Control Tuning** page is a dedicated experiment surface for tuning an already selected structure. Generic Scope/Bode/FFT remain separate expert analysis tools.

The first Control Tuning layout uses three functional regions:

```text
+------------------------------------------+----------------------+
|                                          | Current Loop         |
|                                          | Bandwidth [1000] Hz  |
|                                          | Id Kp      read-only |
|                                          | Id Ki      read-only |
|              Experiment Waveform         | Iq Kp      read-only |
|                                          | Iq Ki      read-only |
|                                          |                      |
|                                          | Speed Loop           |
|                                          | Bandwidth [50] Hz    |
|                                          | Kp         read-only |
|                                          | Ki         read-only |
|                                          |                      |
|                                          | Position Loop        |
|                                          | Kp         [5.0]     |
+------------------------------------------+                      |
| Motion Command                           | Mechanical Observer  |
| Mode / target / speed / acc / dec        | Bandwidth [100] Hz   |
|                         [ Run ]           |                      |
+------------------------------------------+----------------------+
```

The right column edits **design parameters** rather than exposing every internal coefficient as an independent tuning knob. The firmware derives active gains from the current motor model and the selected design parameter. Active gains are shown read-only so the engineer can see exactly what is running:

```text
Current bandwidth  --> Id/Iq Kp, Ki
Speed bandwidth    --> Speed Kp, Ki
Position Kp        --> Position controller Kp
Observer bandwidth --> Mechanical ESO observer gains
```

A tuning edit is staged by the GUI and applied as part of the next experiment. The first implementation should not continuously rewrite controller coefficients while the motor is running. During an active run the tuning controls are locked.

One **Run** represents one bounded experiment:

```text
apply dirty tuning parameters
        |
        v
read back active values/gains
        |
        v
configure motion command
        |
        v
start experiment capture
        |
        +-- pre-capture, default about 0.5 s
        |
        v
enable / run motor
        |
        v
execute requested motion
        |
        v
motion complete / settling condition
        |
        +-- post-capture, default about 1.0 s
        |
        v
stop capture and keep the waveform on screen
```

Pre/post capture are part of the experiment because startup response and post-arrival vibration are often the quantities being tuned. Capture does not stop at the exact instant the target is reached.

The waveform panel is therefore not an indefinitely scrolling Scope. It starts with the experiment, stops automatically after the post-capture window, and preserves the completed record until the next run or explicit clear. It should reuse the shared acquisition/channel/uPlot infrastructure instead of creating a second telemetry implementation.

Default channels depend on the experiment mode and may still be customized. Typical defaults are:

```text
Position experiment:
  Position Ref / Encoder Position
  Wm Ref / Mechanical ESO Wm
  Iq Ref / Iq
  ESO disturbance estimate

Speed experiment:
  Wm Ref / Mechanical ESO Wm
  Encoder differentiated Wm (diagnostic)
  Iq Ref / Iq
  ESO disturbance estimate

Current experiment:
  Id Ref / Id
  Iq Ref / Iq
  Ud / Uq
```

Motion Command fields are mode-specific rather than one universal form:

- Position: target position, maximum speed, acceleration, deceleration.
- Speed: target speed, acceleration, deceleration, hold time.
- Torque/current-oriented tests: target value and hold time.

The GUI composes this workflow through the Application API. It does not write protocol frames directly. The Application layer owns sequencing, capture lifetime, motor actions and completion/error handling so the same experiment can later be invoked from GUI, CLI or automation.

### Analysis shares acquisition and plotting infrastructure

Scope, FFT and Bode are related analysis functions, but not identical workflows. They share channel metadata, acquisition buffers, plotting, cursors, units and export infrastructure where practical.

- **Scope** is the time-domain live/capture view.
- **FFT** analyzes acquired data in the frequency domain.
- **Bode** may own an excitation-and-measurement workflow, while still reusing acquisition and plot infrastructure.

The GUI must not open its own transport or decode telemetry wire frames for any of these functions. Acquisition remains an Application Runtime capability.

Chart interaction should use uPlot capabilities and plugins instead of recreating generic plotting behavior in Svelte. Unit metadata should drive reusable scale/axis policy rather than page-specific conditionals.

### Connection is transport-oriented

Connection UI is a functional domain because the product will support more than one transport. Serial, native CAN FD and future transports present transport-specific configuration but converge on the same device/session API above them. The rest of the GUI must not assume that a connected device is represented by a serial port string.

Connection state is global, but connection control belongs only to the Connection page. Scope, Motor, Control and other workflow pages consume the current Device Session; they do not embed their own Connect/Disconnect UI.

## Concurrency and ownership

The runtime owns the transport and demultiplexes received frames into responses, events and streams.

```text
Transport reader
      |
      v
Frame decoder
      |
      +--> transaction router --> request futures
      +--> event bus -----------> application subscribers
      +--> stream router --------> scope / recorder / clients
```

Only one component reads from a transport. Request code never performs its own competing read loop.

## Public API stability

Internal Rust module layout may evolve, but the external Application API is treated as a product contract. Breaking API changes should be intentional and versioned.

Raw/debug access may exist for engineering use, but normal clients should use typed semantic endpoints.

## Testing strategy

- `wire`: deterministic unit tests for framing, DLC, padding and stream resynchronization.
- `protocol`: request/response and event routing tests using an in-memory fake transport.
- `device`: typed Parameter/Action tests against scripted protocol fixtures.
- `application`: session/task/resource-arbitration tests without physical hardware.
- hardware tests: explicit integration tests, never the only validation path.

This separation is important so most regressions can be caught without connecting a motor controller.

## Architecture decisions

Long-lived or non-obvious architecture choices are recorded under `docs/adr/` as Architecture Decision Records. An ADR records context, decision and consequences so future refactors do not accidentally remove an important boundary.

## Current firmware adapter

The initial implementation follows the current AxDr_L wire protocol:

- 11-bit standard CAN identifier;
- message type in bits 10..6 and node in bits 5..0;
- canonical CAN FD data lengths;
- USB byte-stream envelope: `AXDR` + little-endian CAN ID + data length + complete CAN FD data field;
- Parameter and Action transported via the Parameter message family;
- finite Actions use immediate response plus asynchronous ACTION_COMPLETE event;
- FAST/NORMAL telemetry are treated by the host as streams, not as GUI-only Plot features.

`AxDr_L_Motor/tools/*.py` are not architectural dependencies.
