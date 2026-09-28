# Integration Verification Matrix

> **Integration execution/status document.** This file tracks the current end-to-end
> verification pass. It does not redefine product behavior or architecture. When a
> checklist result disagrees with a normative document, investigate/fix the
> implementation unless an explicit product decision changes the requirement.
>
> Normative references: `ARCHITECTURE.md`, `UI_INTERACTION_RULES.md`, and the
> corresponding page design document. Documentation authority rules are in
> `docs/README.md`.

## Verification rules

All items intentionally start unchecked for this integration pass. Existing code,
an existing UI control, or a passing isolated unit test does not by itself mean the
feature is integrated.

- [ ] Mark an item complete only after the stated behavior has been exercised in the current verification pass.
- [ ] A page is not considered verified until its UI -> Application -> device -> authoritative feedback -> UI path is closed.
- [ ] When a failure is observed on one page, identify the shared concept and all consumers before changing code.
- [ ] Fix the component that owns the broken semantic; do not default to patching the page where the symptom appeared.
- [ ] After a shared-layer fix, rerun the affected checks on every consumer listed in the issue impact set.
- [ ] Preserve established Stop/Run/Enable, Parameter, acquisition, persistence, reconnect, and safety semantics while fixing integration bugs.
- [ ] Record real-device verification separately from mock/unit/build verification.

### Completion levels

For each domain:

- [ ] **Wired** — the complete UI/Application/device/feedback path is connected.
- [ ] **Verified** — the normal real-device workflow has been exercised successfully.
- [ ] **Regression Guarded** — the important shared semantics are protected by automated tests where practical.

## Current verification order

Shared foundations come first so later page failures can be attributed correctly.

- [ ] 01 Connection / session lifecycle
- [ ] 02 Parameter state / persistence / runtime telemetry
- [ ] 03 Analysis / Scope
- [ ] 04 Motor / Identification
- [ ] 05 Encoder / Phase Search / Set Zero
- [ ] 06 Motion
- [ ] 07 Control Architecture
- [ ] 08 Control Tuning
- [ ] 09 Limits / Safety
- [ ] 10 Events / Problems
- [ ] 11 Automation
- [ ] 12 Cross-page / reconnect / teardown regression pass

---

## 01 Connection / session lifecycle

**Primary owner:** `ApplicationSession`, `DeviceSession`, desktop connection adapter  
**Shared consumers:** every page, Automation, Scope/Tuning, global toolbar  
**Normative references:** `ARCHITECTURE.md`, `UI_INTERACTION_RULES.md`

### Code review pass — 2026-09-28

Reviewed against `feat/motion-cli-headless @ dbb159c`. A checked `[CODE]`
item means the current source path was traced end-to-end in this pass. It is not a
claim that the real controller/WebView behavior has been exercised.

Important findings:

- desktop Connect owns one `ApplicationSession` / one `DeviceSession`; expert/raw
  CLI utilities are separate tools and are not used by the desktop;
- backend Connect performs the authoritative device `parameter_refresh_all()`
  before returning the connection;
- frontend `connectParameters()` then mirrors `parameter_cached_many` from the
  Application cache, so it does **not** perform a second full device read;
- connection starts only the connection-owned Plot baseline; it does not create a
  user Scope recording or issue Motor Enable/Run/Identification/Motion Actions;
- reconnect stale-result fencing exists independently in Parameter epoch,
  RuntimeTelemetry revision, Problems revision, Automation epoch/revision and
  Scope view revision/connection identity;
- Automation cancellation cleanup and connection teardown may both request the
  same safety Stop, but both use the same `ApplicationSession` / `MotorGate`;
  there is no second transport or competing motor-command path.

### Connect

- [x] **[CODE]** Port refresh is wired `ConnectionPage.refreshPorts -> device_list -> ApplicationSession::available_usb_ports -> UsbCdcTransport::available_ports`.
- [ ] **[HW]** The connected controller appears as the expected USB CDC endpoint on the real host.
- [x] **[CODE]** Selecting/typing a port is preserved by the parent-owned `connectionPort` binding across Connection-page remounts.
- [x] **[CODE]** HostSchema path is parent-owned and preserved across page navigation.
- [x] **[CODE]** HostSchema path is persisted/restored through `localStorage`.
- [ ] **[WEBVIEW]** Restart the packaged/dev desktop and confirm the last HostSchema path is actually restored.
- [x] **[CODE]** Desktop Connect opens one Application/Device session after first tearing down any existing session.
- [x] **[CODE]** Initial device Parameter Read completes and every returned read result is checked before Connect proceeds.
- [x] **[CODE]** Frontend Parameter initialization mirrors the Application cache rather than issuing a second full device read.
- [x] **[CODE]** Plot capabilities are discovered through the device `PLOT_CAPS` path rather than hard-coded in the page.
- [x] **[CODE]** Runtime baseline acquisition starts only after initial Parameter Read and capability discovery.
- [x] **[CODE]** Runtime baseline startup uses Plot operations only; it does not issue a Motor Action.
- [x] **[CODE]** Runtime baseline IDs are built from available NORMAL-capable baseline symbols and returned in `ConnectionDto.runtimeChannelIds`.
- [x] **[CODE]** Connection page renders the returned endpoint, FAST/NORMAL rates, channel limits and FAST block size.
- [x] **[CODE]** Connecting does not implicitly Enable, Run, start Identification, start Motion, or create a user Scope recording.
- [ ] **[HW]** Real Connect completes with the intended controller and the displayed endpoint/capabilities match the controller.

### Disconnect / reconnect

- [x] **[CODE]** Disconnect closes workflow access, closes/cancels the MotorGate, requests Tuning stop, issues the established motor Stop, joins Tuning and waits for controlled stop.
- [x] **[CODE]** Disconnect cancels Automation and waits for task-owned cleanup; Automation uses the same ApplicationSession rather than a second transport/session.
- [x] **[CODE]** Task-owned cleanup is allowed after workflow cancellation/close while new Automation operations are rejected.
- [x] **[CODE]** Scope shutdown stops the shared Plot source; final Application/DeviceSession drop shuts down and joins the device-session/Scope workers.
- [x] **[CODE]** Disconnect during Tuning uses the established Tuning stop/join path before shared acquisition shutdown.
- [x] **[CODE]** Backend removes the active Application only when teardown succeeds.
- [x] **[CODE]** Frontend calls `onDisconnected` only after successful backend disconnect; existing regression test covers failed/success/double-submit UI behavior.
- [x] **[CODE]** Reconnect creates a new frontend Parameter epoch and independent revision fences for RuntimeTelemetry, Problems and Automation.
- [x] **[CODE]** Scope resets on ConnectionInfo identity change and invalidates pending snapshots through `viewRevision`.
- [x] **[CODE]** Motion host configuration persists in `DesktopState.motion`; reconnect resets only Motion runtime/repeat state.
- [x] **[CODE]** Parameter cache/persistence, RuntimeTelemetry, Problems projection, Automation projection and Scope page state are cleared/reset for the new connection.
- [ ] **[HW]** Disconnect while Scope is active leaves no observable stale stream/worker behavior on reconnect.
- [ ] **[HW]** Disconnect while Tuning is active performs controlled motor cleanup and reconnects cleanly.
- [ ] **[HW]** Disconnect while Automation owns motion/scope cleans the task and reconnects without delayed commands from the old session.
- [ ] **[HW]** Force a teardown failure/timeout and confirm the connected UI/session is retained and remains recoverable.

### Exit criteria

- [x] **Wired — code-reviewed.**
- [ ] **Verified — requires current real-device/WebView pass.**
- [ ] **Regression Guarded — relevant tests exist, but they have not been executed in this current pass.**

---

## 02 Parameter state / persistence / RuntimeTelemetry

**Primary owner:** `ParameterService`, shared frontend Parameter state, RuntimeTelemetry projection  
**Shared consumers:** Motor, Encoder, Limits, Control, Motion, Tuning, Parameters, status bar, Automation  
**Normative references:** `ARCHITECTURE.md`, `UI_INTERACTION_RULES.md`, `SHARED_PARAMETER_STATE.md`, `SHARED_ACQUISITION.md`

### Shared Parameter source

- [ ] Connection initial Read populates the shared Parameter state once.
- [ ] Generic Parameters page and all business pages show the same committed value for the same Parameter.
- [ ] Typing edits only the local draft.
- [ ] Enter performs one RAM write and canonical readback.
- [ ] Escape discards the draft and restores current committed RAM value.
- [ ] Blur with an uncommitted draft does not write the device.
- [ ] Enum/select/checkbox controls use the same Parameter write path as numeric editors.
- [ ] A successful write updates every page that presents the affected Parameter.
- [ ] Dependent readback comes from generated HostSchema metadata, not page-specific dependency code.
- [ ] Write/readback errors invalidate/reconcile state without reviving stale values.
- [ ] Explicit Read refreshes device RAM into the shared state.
- [ ] Action completion readback reaches the same shared state before dependent page logic consumes results.

### RAM modified / Save

- [ ] Successful RAM write marks the Parameter modified relative to the persisted baseline.
- [ ] Navigating between pages preserves the modified indication.
- [ ] Writing the value back to the saved baseline clears that Parameter's modified indication.
- [ ] Save is available only when firmware exposes persistence.
- [ ] Save uses firmware persistence semantics rather than a Host replay of selected values.
- [ ] Successful Save establishes the current RAM values as the new frontend presentation baseline.
- [ ] Failed Save does not clear modified indications.

### RuntimeTelemetry baseline

The connection-owned baseline is the shared NORMAL 1 kHz source for:

`PARAM_RUN_IQ`, `PARAM_RUN_WM`, `PARAM_RUN_POSITION`, `PARAM_ADC_VBUS`.

- [ ] Baseline starts on connection and remains active independently of Scope Run/Stop.
- [ ] Current status value comes from baseline Iq latest sample.
- [ ] Speed status value comes from baseline mechanical speed latest sample.
- [ ] Position status value comes from baseline Plot total-turn latest sample.
- [ ] Vbus status value comes from baseline Vbus latest sample.
- [ ] Bottom status bar displays Current / Speed / Position / Vbus in the established order.
- [ ] Status-bar display refresh uses the shared desktop scheduler and does not create independent timers.
- [ ] RuntimeTelemetry reads are Host-memory reads and do not issue periodic Parameter requests.
- [ ] Plot position is used only for runtime presentation/history and never overwrites the exact typed Position Parameter.
- [ ] Motor State remains on its authoritative Parameter path and is not inferred from telemetry.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 03 Analysis / Scope

**Primary owner:** `SharedAcquisition`, `MixedScopeSession`, Scope Application API, shared ECharts view  
**Shared consumers:** Scope page, Control Tuning, RuntimeTelemetry, Automation Scope operations  
**Normative references:** `SCOPE_PAGE.md`, `SHARED_ACQUISITION.md`, `ARCHITECTURE.md`

### Baseline preview semantics

- [ ] Immediately after connection the physical baseline stream is LIVE.
- [ ] Immediately after connection the user-owned Scope recording is STOPPED.
- [ ] Entering Scope before the first Run can display selected baseline NORMAL history.
- [ ] Baseline preview is explicitly distinguishable from a real Scope recording.
- [ ] Scope button still shows Run while baseline preview is rolling.
- [ ] Selected baseline NORMAL Iq is visibility-only.
- [ ] Selected baseline NORMAL Speed is visibility-only.
- [ ] Selected baseline NORMAL Position is visibility-only.
- [ ] Selected baseline NORMAL Vbus is visibility-only.
- [ ] Hiding all baseline traces does not stop the baseline stream.
- [ ] Showing/hiding a baseline NORMAL trace does not send unnecessary Plot Config/Start/Stop.
- [ ] Switching the same baseline-capable channel from NORMAL to FAST turns it into an explicit Scope demand.

### Channel configuration

- [ ] Available channel list is generated from device Plot capabilities.
- [ ] Unsupported FAST/NORMAL choices are disabled.
- [ ] Merged baseline + Scope + Tuning demand respects device FAST/NORMAL channel limits.
- [ ] Duplicate Parameter IDs are transmitted only once.
- [ ] FAST wins when multiple consumers request the same ID at different rates.
- [ ] NORMAL consumers of a FAST source receive valid integer decimation rather than interpolation.
- [ ] Changing a non-baseline channel while Scope is stopped does not destroy the frozen record.
- [ ] Hot reconfigure while Scope is LIVE does not freeze/crash the UI.
- [ ] Reconfigure does not leave an old Config_ID/layout feeding the new record.

### Run / Stop recording

- [ ] First Run creates the first independent Scope recording.
- [ ] First Run changes Scope state STOPPED -> LIVE.
- [ ] First Run resets horizontal view to Follow Latest with the default 0.5 s span.
- [ ] New Run seeds available recent baseline history for matching baseline NORMAL channels.
- [ ] Recorded duration increases while Scope is LIVE.
- [ ] New samples append to the Scope record while LIVE.
- [ ] Stop changes Scope LIVE -> STOPPED.
- [ ] Stop freezes Scope samples and loss count.
- [ ] Stop does not issue Motor Stop.
- [ ] Stop does not stop baseline RuntimeTelemetry.
- [ ] Stop does not stop an independently active Tuning recording.
- [ ] After Stop, newer baseline samples do not overwrite/substitute the frozen Scope record.
- [ ] Second Run creates a new Scope record instead of resuming/appending to the previous frozen record.

### Viewer / interaction

- [ ] ECharts displays every selected series with the correct unit and stable channel identity.
- [ ] Per-channel Scale/div changes only presentation, not acquisition values.
- [ ] Per-channel Y position changes only presentation.
- [ ] Auto vertical scaling uses captured data and remains independent per channel.
- [ ] Horizontal mouse/trackpad zoom is continuous.
- [ ] Horizontal pan navigates retained history.
- [ ] Pan/zoom leaves Follow Latest.
- [ ] Latest returns the current span to newest data.
- [ ] Stop preserves pan/zoom and per-channel vertical settings.
- [ ] Lost-frame count is visible and changes only according to acquisition loss.
- [ ] Page switching does not start/stop the connection-owned baseline.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 04 Motor / Identification

**Primary owner:** ParameterService, MotorActionService, Identification semantic Application API  
**Shared consumers:** Motor page, Parameters, Problems, Automation, global motor controls  
**Normative references:** `MOTOR_PAGE.md`, `APPLICATION_PREFLIGHT.md`, `UI_INTERACTION_RULES.md`

### Motor parameters

- [ ] Motor model Parameters display current shared values after connection.
- [ ] Editing pole pairs/Rs/Ld/Lq/Flux/J/B follows common Enter/Esc/blur semantics.
- [ ] Motor model write dependent readbacks update Control/Limits values through HostSchema metadata.
- [ ] Parameter page immediately reflects Motor-page committed writes.
- [ ] Save/modified-state semantics match every other Parameter-backed page.

### Rs/Ls

- [ ] Preflight checks the current authoritative prerequisites.
- [ ] Start does not silently create a second Enable/Run semantic.
- [ ] Required Enable confirmation follows the established workflow.
- [ ] Running state is visible.
- [ ] Finite Action completion is observed.
- [ ] Successful result Parameters are read back.
- [ ] Failure is surfaced explicitly.
- [ ] Apply uses the semantic identification Apply path.
- [ ] Successful Apply updates Active/shared Motor Parameters.
- [ ] Global Stop immediately invokes the shared motor Stop path during the operation.

### Flux

- [ ] Same preflight/Enable/Run/Stop semantics as the common identification workflow.
- [ ] Rs/Ls prerequisites use currently active values.
- [ ] Result validity and result value are read back after completion.
- [ ] Apply updates the shared active Flux Parameter.
- [ ] Failure leaves the application in an observable recoverable state.

### J/B

- [ ] J/B is only available when firmware capability exists.
- [ ] Flux prerequisite is checked through the established preflight path.
- [ ] J/B excitation Parameters come from shared Parameter state.
- [ ] Completion/result/Apply semantics match other identification types.
- [ ] J/B failure does not leave motor motion owned by an abandoned workflow.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 05 Encoder / Phase Search / Set Zero

**Primary owner:** Encoder Parameters, PreflightService, semantic phase-search/set-zero Application API  
**Shared consumers:** Encoder page, Motion, status bar Position, Problems, Automation  
**Normative references:** `ENCODER_PAGE.md`, `APPLICATION_PREFLIGHT.md`, `UI_INTERACTION_RULES.md`

### Encoder configuration

- [ ] Protocol selector shows only values exposed by HostSchema.
- [ ] Selecting protocol writes RAM immediately through shared Parameter API.
- [ ] Parameter page reflects the committed protocol value.
- [ ] Protocol-specific fields appear/disappear correctly.
- [ ] SPI model selection writes/readbacks correctly.
- [ ] Encoder direction follows the configured firmware Parameter.
- [ ] Encoder configuration writes refresh related calibration/feedback Parameters through HostSchema metadata.
- [ ] Changing protocol to NONE and back does not freeze the Scope or application UI.

### Feedback

- [ ] Valid encoder movement changes the global runtime Position display.
- [ ] Valid encoder movement changes the global runtime Speed display through the intended feedback chain.
- [ ] Invalid/faulted encoder state is not represented as a permanent duplicate Ready/Valid table when Problems owns the fault presentation.
- [ ] Encoder fault/problem state clears only when authoritative state clears/rechecks.

### Phase Search

- [ ] Preflight checks Encoder Ready/Valid/Fault, pole pairs, current limit and search current.
- [ ] Phase Search can request the established explicit Enable flow where required.
- [ ] Start uses the semantic Application phase-search composition.
- [ ] Global Stop invokes the same motor Stop semantic; no separate page-local Abort lifecycle exists.
- [ ] Completion event is observed.
- [ ] Successful result is automatically applied according to the established page contract.
- [ ] Offset/direction/calibration Parameters reflect the successful result in shared state.
- [ ] Failure surfaces an actionable error/problem without silently altering unrelated settings.

### Set Zero

- [ ] Set Zero uses the semantic Application API.
- [ ] Set Zero updates the exact typed position/reference semantics expected by firmware.
- [ ] Global runtime Position reflects the new zero on the baseline stream.
- [ ] Parameter/shared views update after the immediate action readback.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 06 Motion

**Primary owner:** MotionService + Motion Parameter view + semantic Motion Run/Stop  
**Shared consumers:** Motion page, Control Tuning, Automation, Scope/RuntimeTelemetry  
**Normative references:** `MOTION_PAGE.md`, `UI_INTERACTION_RULES.md`

### Mode and command state

- [ ] Available Motion modes follow firmware/Application capabilities.
- [ ] Mode changes are allowed only in the established motor state.
- [ ] Mode write is a shared Parameter write and appears on Parameters page.
- [ ] Torque target writes the correct Parameter.
- [ ] Speed target writes the correct Parameter.
- [ ] Absolute Position target uses the exact typed Position Parameter.
- [ ] Incremental Position delta remains explicit Host-owned command state and is not disguised as an uncommitted Parameter.
- [ ] Sensorless-only fields appear only in Sensorless Speed mode when firmware exposes them.

### Trajectory configuration / preview

- [ ] Wm Max/Acc/Dec values come from shared Parameter state.
- [ ] Preview reads committed Parameter values rather than silently committing drafts.
- [ ] Preview uses current limits/capabilities according to the established Application semantics.
- [ ] Position preview matches absolute/incremental command semantics.
- [ ] Speed preview contains no stale Position-only validation.
- [ ] Unsupported S-curve/Filtered controls are not presented as executable capabilities.

### Run / Stop / Repeat

- [ ] Run never implicitly Enables the motor.
- [ ] Run is available only when the established prerequisites are met.
- [ ] Run sends the semantic Motion operation once.
- [ ] Physical motor response matches the selected mode/target.
- [ ] Runtime Speed/Position/Current feedback reflects motion through the shared baseline.
- [ ] Stop immediately invokes shared motor Stop semantics.
- [ ] Controlled deceleration may leave motor state RUN temporarily without making Host Stop appear failed.
- [ ] Repeat starts one commanded leg and then the reverse leg according to the established repeat rule.
- [ ] Repeat direction advances only after successful completion.
- [ ] Stop/cancel prevents a delayed repeat leg from starting.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 07 Control Architecture

**Primary owner:** shared Parameter state + Control Architecture presentation  
**Shared consumers:** Control page, Control Tuning, Parameters  
**Normative references:** `CONTROL_ARCHITECTURE_PAGE.md`, `UI_INTERACTION_RULES.md`

### Current Loop

- [ ] Gain Source selector reflects firmware enum symbols, not hard-coded ordinal assumptions.
- [ ] Bandwidth mode values write/read back correctly.
- [ ] Manual Id/Iq Kp/Ki values write/read back correctly.
- [ ] Model-dependent gain readbacks update automatically after relevant Motor Parameter changes.
- [ ] Current Loop values match the same Parameters shown in Control Tuning/Parameters.

### Speed Loop

- [ ] Gain Source selector reflects firmware enum symbols.
- [ ] Bandwidth and Kp/Ki write/readback correctly.
- [ ] Mechanical ESO bandwidth writes/readbacks correctly.
- [ ] Speed-loop feedback/observer presentation does not claim unsupported signal semantics.
- [ ] Values stay synchronized with Control Tuning and Parameters.

### Position Loop

- [ ] Position-loop parameters come from shared Parameter state.
- [ ] Kp editing follows common Parameter semantics.
- [ ] Unsupported controller choices remain capability placeholders rather than fake selectable algorithms.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 08 Control Tuning

**Primary owner:** Tuning experiment orchestration + Motion semantic API + SharedAcquisition + shared Scope ECharts viewer  
**Shared consumers:** Control Tuning, Scope infrastructure, Motor Stop, RuntimeTelemetry  
**Normative references:** `CONTROL_TUNING_PAGE.md`, `SCOPE_PAGE.md`, `UI_INTERACTION_RULES.md`

### Page state / Parameters

- [ ] Entering the page shows current committed Parameters immediately.
- [ ] Editing a tuning Parameter uses the same shared Parameter editor semantics.
- [ ] Tuning page does not own a second copy of current/speed/position gains.
- [ ] Controller source/mode changes remain synchronized with Control Architecture.

### Experiment

- [ ] Run does not implicitly Enable the motor.
- [ ] Run uses committed Motion/Tuning settings only.
- [ ] Default acquisition channels exist and match the intended loop experiment.
- [ ] Experiment recording uses SharedAcquisition, not a second transport/decoder.
- [ ] Starting Tuning does not overwrite a frozen normal Scope record.
- [ ] Waveform data appears while the experiment is active.
- [ ] Experiment completion preserves the final waveform.
- [ ] Tuning Stop invokes the shared motor Stop semantic.
- [ ] Tuning Stop does not corrupt connection-owned RuntimeTelemetry.
- [ ] Failed/cancelled experiment performs the established motor cleanup.
- [ ] Repeat experiment executes one Run and one reverse Run according to the documented semantics.

### Viewer

- [ ] Tuning waveform reuses `ScopeEchartsView`.
- [ ] Per-channel Y controls work independently.
- [ ] Horizontal zoom/pan behaves consistently with Scope where the workflows overlap.
- [ ] Tuning-specific compact layout does not fork acquisition/plot semantics.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 09 Limits / Safety

**Primary owner:** firmware Parameters + shared Parameter state; GUI only presents authoritative values  
**Shared consumers:** Limits, Motor/Identification preflight, Motion, Problems  
**Normative references:** `LIMITS_PAGE.md`, `UI_INTERACTION_RULES.md`

### Operating limits

- [ ] User current limit is editable through shared Parameter API.
- [ ] User speed limit is editable through shared Parameter API.
- [ ] Effective current comes directly from `PARAM_LIMIT_I_EFFECTIVE`.
- [ ] Effective speed comes directly from `PARAM_LIMIT_WM_EFFECTIVE`.
- [ ] GUI never recomputes Effective from User/Hardware values.
- [ ] Writing a user limit triggers the HostSchema-defined dependent Effective readback.
- [ ] Writing Motion Wm Max triggers the HostSchema-defined speed Effective readback.
- [ ] Hardware limit displays only when firmware exposes an authoritative read-only Parameter.
- [ ] Missing Hardware limit capability is displayed as unavailable, not synthesized.

### Bus voltage

- [ ] Actual Vbus uses the same baseline RuntimeTelemetry/Parameter source without a page-local poll.
- [ ] Vbus Min displays/edits only when firmware exposes it.
- [ ] Vbus Max displays/edits only when firmware exposes it.
- [ ] GUI out-of-window styling does not replace firmware protection authority.

### Position limits

- [ ] Position-limit section remains unavailable/muted while firmware capability is absent.
- [ ] No synthetic zero-valid/position-limit values are written.
- [ ] When capability exists, zero dependency and enable semantics follow the normative page contract.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 10 Events / Problems

**Primary owner:** Application ProblemService + Protection event/Parameter state  
**Shared consumers:** toolbar Problems indicator, Events page, Encoder/Motor/Limits fault presentation  
**Normative references:** `EVENTS_PAGE.md`, `UI_INTERACTION_RULES.md`

### Event source / lifecycle

- [ ] Firmware `EVENT_NOTIFY` is received without changing the raw DeviceSession event surface.
- [ ] Application decodes Report/Warning/Error/Trip masks.
- [ ] Event snapshot updates the shared `PARAM_EVENT_*` cache without extra device reads.
- [ ] Active problem set reflects current authoritative protection masks.
- [ ] Resolving a protection condition removes it from Active.
- [ ] Resolved record remains in History.
- [ ] Repeated/new mask occurrences update count/timestamps consistently.
- [ ] Clear History deletes resolved history but leaves active problems.
- [ ] Recheck explicitly rereads the four Protection Parameters.
- [ ] Clear Fault uses semantic Protection Clear and then rechecks.

### UI

- [ ] Top-right badge count equals active problem count.
- [ ] Top-right icon reflects highest active severity.
- [ ] Popover shows only the highest-priority concise problem summary.
- [ ] Clicking the summary opens Events.
- [ ] Events page shows Active separately from History.
- [ ] Raw mask is visible while per-bit Host metadata is unavailable.
- [ ] GUI does not invent specific PROT_* names that are not supplied by an authoritative Host contract.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 11 Automation

**Primary owner:** AutomationRuntime + SessionWorkflowApi + the same ApplicationSession used by GUI  
**Shared consumers:** Parameter, Motor, Motion, Identification, Scope, Tuning  
**Normative references:** `AUTOMATION_PAGE.md`, `ARCHITECTURE.md`

### Ownership / execution

- [ ] Script executes through the current ApplicationSession rather than opening another transport.
- [ ] Script does not call `nmixxctl` as its motor-control implementation.
- [ ] Script source preview matches the contents actually executed.
- [ ] Only one active Automation task owns conflicting workflow mutations.
- [ ] Page navigation does not stop the running task.
- [ ] Logs/task state remain visible after leaving/re-entering Automation.
- [ ] Cancellation prevents later scripted Run from starting.
- [ ] Disconnect prevents stale script operations from reaching a reconnected session.

### Shared semantic operations

- [ ] Parameter set uses shared write + dependent readback semantics.
- [ ] Config Save preserves the exact successful post-save baseline.
- [ ] Motor Enable/Disable/Stop use semantic Application methods.
- [ ] Motion Run uses the same Motion service as GUI.
- [ ] Identification uses the same preflight/start/completion/apply semantics as GUI.
- [ ] Phase Search uses the same semantic Application API.
- [ ] Scope operations use SharedAcquisition.
- [ ] Tuning operations use the existing Tuning experiment runtime.
- [ ] Script cleanup stops only task-owned motion/scope resources.
- [ ] Read-only script cancellation does not stop unrelated motor motion or Scope recording.

### Failure handling

- [ ] Interpreter launch failure is explicit.
- [ ] Timeout is explicit.
- [ ] Abnormal exit is explicit.
- [ ] Excess output remains bounded.
- [ ] Cleanup failure is reported and not converted into success.
- [ ] Motor controlled-stop timeout remains observable.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## 12 Cross-page / reconnect / teardown regression pass

This section is intentionally cross-domain. It catches bugs caused by fixing one
page at the wrong ownership layer.

### Shared Parameter regression

- [ ] Change one Motor Parameter; Motor, Parameters, Control/Limits dependent views all agree.
- [ ] Change one Control tuning Parameter; Control Architecture and Parameters agree.
- [ ] Change one Motion limit; Motion and Limits Effective agree after readback.
- [ ] Change Encoder configuration; Encoder/shared calibration/feedback views agree.
- [ ] Save clears the same modified state on every page.

### Shared acquisition regression

- [ ] Status-bar Current/Speed/Position/Vbus continue while Scope page is hidden.
- [ ] Scope baseline preview does not create a Scope record.
- [ ] Scope Stop freezes only Scope.
- [ ] Tuning experiment does not overwrite frozen Scope history.
- [ ] Scope Run during/after Tuning respects merged device demand.
- [ ] FAST/NORMAL changes do not duplicate Parameter IDs.
- [ ] RuntimeTelemetry does not regress when Scope selection changes.

### Global motor semantics regression

- [ ] Global Stop is available whenever connected, independent of cached motor state.
- [ ] Motor page, Motion, Identification, Phase Search, Tuning and Automation all converge on the same Stop semantics.
- [ ] Disable remains distinct from Stop.
- [ ] No page invents a private Abort/Stop lifecycle that bypasses Application motor Stop.

### Session generation regression

- [ ] Disconnect/reconnect invalidates stale Parameter writes/readbacks.
- [ ] Disconnect/reconnect invalidates stale RuntimeTelemetry projection.
- [ ] Disconnect/reconnect invalidates stale Problem snapshots.
- [ ] Disconnect/reconnect prevents stale Automation operations.
- [ ] Disconnect/reconnect does not resurrect an old Scope/Tuning recording as current live state.

### Build / automated checks

- [ ] `cargo check --workspace`
- [ ] `cargo test --workspace`
- [ ] `cd apps/desktop && npm ci`
- [ ] `npm run build`
- [ ] `npm run test:all`
- [ ] Firmware HostSchema exporter/checks pass for the firmware revision used in the hardware test.
- [ ] Record the firmware commit/SHA used for the final integration pass.
- [ ] Record the NMIXX Motor Studio commit/SHA used for the final integration pass.

### Exit criteria

- [ ] Wired
- [ ] Verified
- [ ] Regression Guarded

---

## Integration issue record template

When a checkbox fails, create a short record before modifying code.

- [ ] **Symptom:** what the user observed.
- [ ] **Observed in:** page/workflow where it was discovered.
- [ ] **Shared concept:** Parameter / RuntimeTelemetry / Action / Motion / acquisition / Problems / session / UI-only.
- [ ] **Owner:** the component that owns the broken semantic.
- [ ] **Other consumers:** every page/client that consumes the same owner.
- [ ] **Root cause:** evidence-based cause, not the visible symptom.
- [ ] **Fix layer:** where the correction belongs.
- [ ] **Must not change:** established behavior that the fix must preserve.
- [ ] **Regression set:** checks/pages that must be rerun after the fix.
- [ ] **Automated guard:** test added/updated when practical.
- [ ] **Hardware result:** exact real-device result after the fix.

Example:

```text
Symptom: Scope Run appears ineffective.
Observed in: Analysis / Scope.
Shared concept: acquisition lifecycle.
Owner: SharedAcquisition.
Other consumers: Scope, Tuning, RuntimeTelemetry, Automation.
Root cause: user Scope record entered LIVE during connection initialization.
Fix layer: Application acquisition.
Must not change: connection-owned 1 kHz baseline; Motor state; frozen Scope history.
Regression set: Scope Run/Stop, status bar telemetry, Tuning, Automation Scope.
```

## Verification run metadata

- [ ] Date:
- [ ] Host OS:
- [ ] NMIXX Motor Studio commit:
- [ ] Firmware repository/commit:
- [ ] Motor:
- [ ] Encoder:
- [ ] DC bus voltage:
- [ ] Connection/transport:
- [ ] HostSchema source:
- [ ] Notes:
