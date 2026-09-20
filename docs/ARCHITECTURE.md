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

`ApplicationSession` is the runtime owner used by normal product clients. It owns one device session plus the loaded Host schema and shared domain services/capabilities for that connection.

```text
GUI -----------+
CLI -----------+--> ApplicationSession --> domain services --> DeviceSession
Automation ----+
```

Normal product clients must not assemble `DeviceSession + ParameterService + ScopeSession + MotionService` themselves. Tauri is a thin IPC bridge over `ApplicationSession`; the formal CLI uses the same runtime. Low-level smoke/protocol tests may intentionally use `DeviceSession` directly because their purpose is to validate the lower layer itself.

Motion is split by ownership rather than by page. Device-owned motion values such as motor mode, trajectory limits and executable targets are ordinary shared Parameters; `ParameterService` is their only host-side source of truth. `MotionService` stores only command semantics that the firmware does not represent as Parameters, such as Absolute/Incremental position interpretation, the incremental delta and the manual alternating Repeat state. Tauri must not keep a second Motion model beside `ApplicationSession`.

The Application API should expose domain concepts such as `parameter.get`, `motor.enable`, `motor.disable`, `motor.stop`, `action.start`, `stream.subscribe` and `task.cancel`, not raw CAN IDs or payload bytes.

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
  +-- Motor
  +-- Encoder
  +-- Limits / Safety
  +-- Control Architecture
  |     +-- Current Loop
  |     +-- Speed Loop
  |     +-- Position Loop
  +-- Control Tuning
  |
  +-- Analysis
  |     +-- shared acquisition
  |     +-- Scope
  |     +-- FFT
  |     +-- Bode
  |     +-- capture/export
  |
  +-- Parameters
  +-- Events
  +-- Automation
```

`App.svelte` is the workbench shell. It owns navigation, global connection summary, global notifications, global motor controls and the status bar. Device and analysis behavior belongs in domain modules.

Shared interaction behavior is defined in `UI_INTERACTION_RULES.md`.

## Workbench-level state and controls

The shell continuously presents application-global context. It may include:

- device connection state;
- motor state;
- compact live position/speed/current feedback;
- global motor `Enable / Disable` control;
- global motor `Stop` control.

Connection configuration and Connect/Disconnect are not duplicated in the shell. They belong to the Connection page because establishing a Device Session may require transport, endpoint and other transport-specific configuration.

Global motor controls follow these semantics:

```text
DISABLED   [ Enable ]   [ Stop ] disabled
ENABLED    [ Disable ]  [ Stop ] disabled
RUN        [ Disable ]  [ Stop ] active
```

`Enable / Disable` is one stateful button in a stable location. Text, icon and color change together to indicate the action that pressing the button will perform. `Stop` is a separate persistent button because stopping current motion/task and changing motor enable state are different operations.

`Run` is not a global shell action. Run always has workflow context: position/speed/torque motion, a tuning experiment, identification internals, Bode excitation, or another domain operation. The page/domain that defines the command owns the Run/Start action.

Other mutually exclusive Start/Stop-style operations should normally use one stateful button in a stable location when both actions operate on the same task/resource. See `UI_INTERACTION_RULES.md`.

## UI reuse boundary

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

## Workflow-oriented navigation

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
- **Encoder** configures feedback protocol/interface and protocol-specific parameters. Phase search current, homing and zero-setting tools belong here because they establish position/electrical alignment.
- **Limits / Safety** configures user operating limits and other software safety boundaries while keeping hardware protection semantics distinct.
- **Control Architecture** is a parent workflow with three child pages: **Current Loop**, **Speed Loop**, and **Position Loop**. Each child configures that loop's control structure: controller type, topology, feedback/observer choice, filters, feedforward and the Parameters that belong to those blocks. It may expose editable Bandwidth/Kp/Ki inside the diagram when those Parameters are part of the active block, but it is not the primary repeated tune-run-inspect workspace. Detailed page rules are in `CONTROL_ARCHITECTURE_PAGE.md`.
- **Control Tuning** runs bounded tuning experiments for the already selected structure: it repeats only the common / primary tuning Parameters, binds them to one motion command, captures the synchronized response, and preserves the completed waveform for comparison. It does not absorb structural options such as controller type, filters, feedback selection or feedforward.
- **Analysis** provides general-purpose continuous or manually triggered engineering analysis. Scope, FFT and Bode share acquisition and plotting infrastructure but are not responsible for the Control Tuning experiment workflow.

Cross-workflow expert tools are visually separated from the commissioning sequence. The generic **Parameters** table, **Events**, and **Automation** entry belong to this secondary group rather than interrupting the primary workflow.

## Parameter is a shared service, not one page

The Parameter subsystem is the single source of truth for host-visible parameters. The generic Parameters page is an expert view over the complete registry, but domain pages reuse the same Parameter API:

```text
Parameter service
      |
      +--> Parameters table   (all parameters, direct read/write/import/export)
      +--> Motor page         (motor-related parameter view)
      +--> Encoder page       (feedback-related parameter view)
      +--> Limits page        (operating/safety limits)
      +--> Control Architecture
      |      +--> Current Loop
      |      +--> Speed Loop
      |      +--> Position Loop
      +--> Control Tuning     (primary tuning parameters and experiments)
      +--> other domain pages
```

A value such as motor resistance must not have separate storage or write paths in the generic table and the Motor page. Both views call the same Application API entry.

Device/Application state remains owned by the Rust Application layer. Svelte pages read committed Parameter values through the shared Parameter API and may keep only view state or uncommitted editing drafts locally. Recreating a page must not introduce a second frontend source of truth for device Parameters.

Uncommitted text drafts remain page-local by design and may be discarded when navigating away.

The Parameters page uses TanStack Table for table behavior. TanStack owns sorting/filtering/table state; NMIXX owns markup, styling, device reads/writes and value editing semantics. Table code must not bypass `ParameterService`.

## Control Architecture and Control Tuning are different workflows

The **Control Architecture** domain is diagram-oriented and contains three child pages rather than one combined three-loop page:

```text
Control Architecture
├─ Current Loop
├─ Speed Loop
└─ Position Loop
```

Each child page represents how that loop is built and allows configuration in the block where each setting acts. The parent is primarily a navigation/grouping concept; it does not require an additional overview page.

Typical responsibilities include:

- controller selection for each loop;
- feedback / observer selection;
- filters and their parameters;
- feedforward blocks;
- anti-windup or other controller-specific structural options;
- Bandwidth / Kp / Ki when those Parameters belong to the selected controller block.

Conceptually:

```text
Speed Ref
   |
   v
[ Acc / Dec ]
   |
   v
  (+) <---------------------------------- Speed Feedback
   |
   v
+-----------------------+
| Speed Controller      |
| Type         [ PI v ] |
| Source       [ ...  ] |
| Bandwidth    [ ...  ] |
| Kp / Ki      [ ...  ] |
+-----------------------+
   |
   v
[ Output Filter ]
   |
   v
  (+) <------------------------- [ Feedforward ]
   |
   v
Iq Ref
```

A Parameter shown here is still the same shared Parameter exposed by the generic Parameters page and any other domain view. Page placement does not create a separate value or write path.

The **Control Tuning** page serves a different workflow: change a small set of frequently adjusted Parameters, run one bounded motion experiment, and inspect the resulting waveform.

The first Control Tuning layout keeps three functional regions:

```text
+------------------------------------------+----------------------+
|                                          | Current Loop         |
|                                          | Source [Bandwidth v]|
|                                          | Bandwidth [1000] Hz |
|              Experiment Waveform         | Id/Iq Kp/Ki [...]   |
|                                          |                      |
|                                          | Speed Loop           |
|                                          | Source [Bandwidth v]|
|                                          | Bandwidth [50] Hz   |
|                                          | Kp / Ki [...]       |
|                                          |                      |
+------------------------------------------+ Position / Observer  |
| Motion Command                           | primary params       |
| mode / target / speed / acc / dec        |                      |
|                         [ Run ]           |                      |
+------------------------------------------+----------------------+
```

The tuning page intentionally repeats some Parameters that also appear inside Control Architecture. This is a workflow convenience, not duplicated state:

```text
Control Architecture ----+
                         +--> ParameterService --> Device Parameter API
Control Tuning ----------+
Parameters page ---------+
```

Typical Parameters shared by Architecture and Tuning are Current/Speed Bandwidth, actual controller Kp/Ki, Position Kp and Mechanical ESO Bandwidth.

Structural controls such as controller type, filter mode/frequency, feedback selection, feedforward and algorithm-specific topology options remain on Control Architecture unless there is a concrete reason they become high-frequency tuning controls.

One **Run** on Control Tuning represents one bounded experiment:

```text
verify no uncommitted tuning draft
        |
        +-- no --> reject and ask the user to commit/discard
        |
       yes
        v
verify motor state == ENABLED
        |
        +-- no --> reject with explicit "Enable motor first"
        |
       yes
        v
read back effective tuning values
        |
        v
configure motion command
        |
        v
start finite capture
        |
        +-- pre-capture
        v
run configured experiment
        |
        v
motion complete / settling condition
        |
        v
return to ENABLED
        |
        +-- post-capture
        v
stop capture and keep waveform
```

There is deliberately no implicit Enable/Disable in the experiment. The global Stop control remains available while an experiment is running and stops the active motion/task through the same Application state used by the page.

The waveform panel is not an indefinitely scrolling Scope. It reuses shared acquisition/uPlot infrastructure but is finite and synchronized to the tuning experiment.

## Motion ownership and execution semantics

Motion is a first-class application domain, but device Parameters and host command semantics have different owners.

```text
                         ParameterService
                              |
          +-------------------+-------------------+
          |                   |                   |
      Motion page       Control Tuning       Parameters page
          |
          +--> device-owned values
               - PARAM_MOTOR_MODE
               - PARAM_MOTION_WM_MAX
               - PARAM_MOTION_WM_ACC
               - PARAM_MOTION_WM_DEC
               - PARAM_TARGET_POSITION
               - PARAM_TARGET_SPEED
               - PARAM_TARGET_TORQUE
               - optional schema-advertised motion Parameters

Application Motion model
          |
          +--> host-only command semantics
               - Absolute / Incremental position interpretation
               - incremental position delta
               - manual alternating Repeat state
```

A device-owned value must not also be stored in `MotionConfig`. Editing the same Parameter from Motion, Control Tuning or the generic Parameters page changes the same active device RAM value and every view observes that shared value.

Numeric device Parameters follow the workbench editor rule defined in `UI_INTERACTION_RULES.md`: typing creates a draft, Enter writes RAM and reads back the canonical value, while Escape or leaving the editor without Enter discards the draft. Enum/select controls such as motor mode may write immediately because one selection is already a complete value.

### Run is execution, not synchronization

Normal Motion Parameters are committed before Run. `motion.run` must not bulk-copy a Host configuration object into device Parameters.

For Speed, Sensorless Speed and Torque, Run normally reduces to validating the required runtime state and starting `ACTION_MOTOR_RUN`.

Absolute Position target is also a normal device Parameter and is written when the user commits the target field.

Incremental Position is intentionally different because the user enters a delta, not an absolute firmware target. At execution time the Application reads the current position, adds the committed host-side delta, writes the resulting `PARAM_TARGET_POSITION`, then starts `ACTION_MOTOR_RUN`.

### Manual Repeat

Repeat is host command semantics, not a firmware Repeat mode and not an automatic loop.

With Position Repeat enabled, the first manual Run establishes endpoints A and B. One Run executes only one leg. A successful Run completion changes which endpoint the **next manual Run** targets:

```text
Run #1: A -> B
Run #2: B -> A
Run #3: A -> B
```

There is no automatic second Run. Stop, Disable, a failed Run or a fault must not advance the Repeat direction. A future continuously cycling servo-test feature is a separate workflow.

### Executable trajectory capability

The GUI exposes only trajectory behavior the connected firmware actually advertises as executable. Current AxDr_L Position/Speed motion uses the T/Trapezoidal planner represented by `Wm_Max / Wm_Acc / Wm_Dec`; unsupported S-curve, filtered-profile and MIT controls are not presented as normal executable Motion options.

If future firmware exposes additional executable trajectory Parameters/capabilities, those controls may appear from schema/capability discovery without introducing parallel Host copies of their values.

The full Motion page and Control Tuning are therefore two workflow projections over the same device Parameters plus the same small Application-owned command semantics. Their difference is presentation density and experiment workflow, not value ownership.

Run, Stop and Disable remain distinct:

- **Run** executes the already committed Motion command.
- **Stop** performs the normal controlled-stop operation exposed by the Application.
- **Disable** removes motor enable and is not an alias for Stop.

Sensorless Speed remains a separate operating mode where the firmware exposes it because startup and observer handover have distinct runtime semantics. Only user-facing Parameters actually exposed by the connected schema are shown.

## Analysis shares acquisition and plotting infrastructure

Scope, FFT and Bode are related analysis functions, but not identical workflows. They share channel metadata, acquisition buffers, plotting, cursors, units and export infrastructure where practical.

- **Scope** is the time-domain live/capture view.
- **FFT** analyzes acquired data in the frequency domain.
- **Bode** may own an excitation-and-measurement workflow, while still reusing acquisition and plot infrastructure.

Detailed Scope interaction rules, including mouse-centered time zoom, history pan, per-channel Y markers and draggable X cursors, are defined in `SCOPE_PAGE.md`.

The GUI must not open its own transport or decode telemetry wire frames for any of these functions. Acquisition remains an Application Runtime capability.

Chart interaction should use the selected ECharts capabilities instead of recreating generic plotting behavior in Svelte. Unit metadata should drive reusable scale/axis policy rather than page-specific conditionals.

## Connection is transport-oriented

Connection UI is a functional domain because the product will support more than one transport. Serial, native CAN FD and future transports present transport-specific configuration but converge on the same device/session API above them. The rest of the GUI must not assume that a connected device is represented by a serial port string.

Connection state is global, but connection configuration and Connect/Disconnect control belong to the Connection page. Scope, Motor, Control and other workflow pages consume the current Device Session; they do not embed their own Connect/Disconnect UI.

Disconnect is session teardown, not a substitute for motor Stop or Disable. Motor safety behavior during communication loss must be defined independently by firmware/Application policy.

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
