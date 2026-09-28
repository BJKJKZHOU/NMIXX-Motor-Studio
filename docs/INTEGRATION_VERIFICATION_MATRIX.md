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

### Code review pass — 2026-09-28

Reviewed against `feat/motion-cli-headless @ 41df381` plus the current source tree.
A checked `[CODE]` item means the ownership/call path was traced in this pass;
it is not a real-device result.

Important findings:

- backend Connection performs the real initial device Read; frontend Parameter
  initialization mirrors `parameter_cached_many`, so there is no duplicate full
  device Read after Connect;
- every business page reviewed uses `selectParameters()` /
  `createParameterEditor()`; committed values therefore come from one frontend
  projection of the Application-owned cache;
- dependent readback IDs are read from HostSchema `metadata.readback`; the old
  handwritten `related_parameter()` table is absent;
- a successful write performs device write -> written/dependent readback -> shared
  cache update before the frontend mirrors that cache;
- a write/readback failure is reflected into cache error state; frontend recovery
  mirrors cache instead of inventing a local fallback value;
- RAM-modified tracking is limited to HostSchema `persistent=true` Parameters;
- firmware `ACTION_PARAMETER_SAVE` runs `NVS_Storage_Save_All()` before sending
  its Action response, so a successful Host response means persistence completed;
- RuntimeTelemetry reads only the connection-owned baseline ring in Host memory;
  its 16 ms UI projection does not send periodic Parameter requests;
- Plot Position remains an f32 total-turn display/history value and is deliberately
  prevented from replacing the exact typed Position Parameter.

### Shared Parameter source

- [x] **[CODE]** Connection initial device Read populates the Application Parameter cache before frontend workflows become available.
- [x] **[CODE]** Frontend connection initialization mirrors the Application cache rather than rereading every Parameter from the device.
- [x] **[CODE]** Generic Parameters page and reviewed business pages consume the same shared Parameter view/editor.
- [x] **[CODE]** Typing edits only the editor-local draft.
- [x] **[CODE]** Enter captures the draft, performs one shared RAM write, and adopts backend canonical readback.
- [x] **[CODE]** Escape discards only the local draft and restores the committed shared value.
- [x] **[CODE]** Blur with an uncommitted draft discards it and does not issue a write.
- [x] **[CODE]** Enum/select/checkbox helpers route complete values through the same shared `commitParameter` / editor select path.
- [x] **[CODE]** Backend write/readback updates the shared Application cache and emits changed IDs to all frontend projections.
- [x] **[CODE]** Dependent readback comes from HostSchema `metadata.readback`, not page-specific dependency rules.
- [x] **[CODE]** HostSchema validates dependent readback targets when the schema is loaded.
- [x] **[CODE]** Read failures replace stale cache success with an explicit error entry instead of reviving an old value.
- [x] **[CODE]** Explicit Read performs a real `parameter_refresh_all` device read and then mirrors the resulting cache.
- [x] **[CODE]** Action completion refresh is Application-owned and frontend action listeners mirror the updated cache before domain handlers consume completion.
- [ ] **[HW]** Edit representative Parameters from Motor, Encoder, Limits, Control, Motion and Parameters pages and confirm all alternate views update to the exact firmware readback value.
- [ ] **[HW]** Force one rejected/invalid write and confirm every view returns to the authoritative device/cache state with a visible error.

### RAM modified / Save

- [x] **[CODE]** HostSchema `persistent` metadata is carried through the Tauri Parameter metadata DTO into frontend state.
- [x] **[CODE]** RAM-modified tracking includes only firmware-declared persistent Parameters.
- [x] **[CODE]** Successful shared Parameter updates are observed by the persistence projection and compared with the saved baseline.
- [x] **[CODE]** Page navigation does not own/reset the persistence projection.
- [x] **[CODE]** Restoring a persistent value to the baseline removes it from the modified set.
- [x] **[CODE]** Save availability is derived from firmware Action capability.
- [x] **[CODE]** Save checks the authoritative motor state and requires DISABLED.
- [x] **[CODE]** Firmware performs `NVS_Storage_Save_All()` before the successful Save Action response.
- [x] **[CODE]** After successful Save, frontend performs authoritative refresh and establishes current persistent RAM values as the new presentation baseline.
- [x] **[CODE]** Failed/unavailable Save cannot reach baseline commit.
- [ ] **[HW]** Change multiple persistent Parameters, confirm modified highlighting across pages, Save, power-cycle/reset, and confirm the persisted values reload.
- [ ] **[HW]** Attempt Save while not DISABLED and confirm firmware rejection does not clear modified highlighting.

### RuntimeTelemetry baseline

The connection-owned baseline is the shared NORMAL 1 kHz source for:

`PARAM_RUN_IQ`, `PARAM_RUN_WM`, `PARAM_RUN_POSITION`, `PARAM_ADC_VBUS`.

- [x] **[CODE]** Baseline acquisition is created by `runtime_start()` and remains independent of user Scope Run/Stop.
- [x] **[CODE]** Current status value comes from baseline Iq latest sample.
- [x] **[CODE]** Speed status value comes from baseline mechanical-speed latest sample.
- [x] **[CODE]** Position status value comes from baseline Plot total-turn latest sample.
- [x] **[CODE]** Vbus status value comes from baseline Vbus latest sample.
- [x] **[CODE]** Bottom status bar presents Current / Speed / Position / Vbus in the established order.
- [x] **[CODE]** Status projection uses the shared `refreshScheduler` rather than a private `setInterval`.
- [x] **[CODE]** RuntimeTelemetry Tauri call reads Host memory only; it does not issue a device Parameter request.
- [x] **[CODE]** Runtime latest-sample lookup reads the ring tail without allocating/copying a snapshot.
- [x] **[CODE]** Plot Position is excluded from scalar Parameter-stream ingestion because its firmware Parameter type is exact `position`.
- [x] **[CODE]** Motor State remains on the authoritative Parameter polling/readback path and is not inferred from RuntimeTelemetry.
- [ ] **[HW]** With the controller connected, confirm Current/Speed/Position/Vbus update continuously while never opening Scope.
- [ ] **[HW]** Move the encoder slowly across turn boundaries and confirm runtime Position presentation is continuous/consistent while exact Position Parameter remains independently readable.
- [ ] **[HW]** Stop/freeze Scope and confirm all four RuntimeTelemetry status values continue updating.

### Exit criteria

- [x] **Wired — code-reviewed.**
- [ ] **Verified — requires current real-device pass.**
- [ ] **Regression Guarded — relevant tests exist, but they have not been executed in this current pass.**

---

## 03 Analysis / Scope

**Primary owner:** `SharedAcquisition`, `MixedScopeSession`, Scope Application API, shared ECharts view  
**Shared consumers:** Scope page, Control Tuning, RuntimeTelemetry, Automation Scope operations  
**Normative references:** `SCOPE_PAGE.md`, `SHARED_ACQUISITION.md`, `ARCHITECTURE.md`

### Code review pass — 2026-09-28

Core acquisition/record ownership is now internally consistent, but this page is
**not yet fully Wired** against the normative Scope design because two viewer
features are still missing from the current ECharts implementation:

- [ ] **[IMPL]** Implement X1/X2 time cursors with X1, X2, Δt and 1/Δt/Hz measurement.
- [ ] **[IMPL]** Implement Scope view-settings persistence for selected channels/rates, per-channel Scale/div and Y Pos, cursor preference and active channel.

The old pre-ECharts `ScopePage.svelte` / `viewSettings.ts` code is not present in
the current branch and must not be revived as a parallel Scope implementation.
These missing features belong in the current `ScopeEchartsPage/ScopeEchartsView`
surface and should use ECharts/ZRender extension points rather than a second plot
interaction engine.

A new transport regression was added in this pass for live Config_ID promotion
during hot reconfiguration. It has not yet been executed in the current workspace.

### Baseline preview semantics

- [x] **[CODE]** Connection-owned baseline is created LIVE while the user Scope record starts STOPPED.
- [x] **[CODE]** Scope snapshot before the first Run projects selected baseline history and marks it `preview = true`.
- [x] **[CODE]** Baseline preview reports Scope state STOPPED; the page therefore keeps the Run action visible.
- [x] **[CODE]** Baseline NORMAL Iq defaults to visibility-only.
- [x] **[CODE]** Baseline NORMAL Speed defaults to visibility-only.
- [x] **[CODE]** Baseline NORMAL Position defaults to visibility-only.
- [x] **[CODE]** Baseline NORMAL Vbus defaults to visibility-only.
- [x] **[CODE]** Hiding all baseline traces leaves the connection-owned baseline demand intact.
- [x] **[CODE]** Showing/hiding an already acquired baseline NORMAL trace can update Scope view config while `SharedAcquisition::apply_demand` detects unchanged device demand and sends no Plot reconfiguration.
- [x] **[CODE]** Selecting the same baseline ID at FAST overrides its NORMAL merged demand and becomes an explicit FAST source.
- [x] **[CODE]** A FAST source feeding a NORMAL baseline consumer is decimated by integer ratio in `Channel::push`, not interpolated.
- [ ] **[HW]** Confirm all four baseline NORMAL traces roll before first Scope Run on the real controller.
- [ ] **[HW]** Toggle baseline trace visibility and verify no observable stream interruption/loss spike.

### Channel configuration / shared demand

- [x] **[CODE]** Available channels and FAST/NORMAL support come from device Plot capabilities.
- [x] **[CODE]** GUI disables unsupported FAST/NORMAL selections.
- [x] **[CODE]** Frontend validates selected demand against device FAST/NORMAL limits including hidden baseline channels.
- [x] **[CODE]** Backend independently validates merged demand against the same device capabilities.
- [x] **[CODE]** `merge_demands` sorts/de-duplicates Parameter IDs; one ID is never transmitted twice.
- [x] **[CODE]** FAST wins when baseline/Scope/Tuning request one ID at different rates.
- [x] **[CODE]** Scope and Tuning consume one `MixedScopeSession` decoder and separate recording buffers.
- [x] **[CODE]** Editing a stopped Scope selection updates the next record layout without mutating the frozen record samples.
- [x] **[CODE]** Live reconfigure uses a pending layout/config marker rather than immediately relabeling old in-flight frames.
- [x] **[CODE]** First frame carrying the new Config_ID promotes the pending layout and resets its sequence tracker.
- [x] **[CODE]** Old-layout frames already in flight remain associated with the active old layout until promotion.
- [x] **[CODE]** Added transport regression for old-frame/new-frame transition during live hot reconfigure without group Stop/Start.
- [ ] **[HW]** Change NORMAL/FAST/channel selections repeatedly while streaming and confirm no UI freeze, UnknownConfig failure or persistent ReconfigurePending state.
- [ ] **[HW]** Confirm actual firmware FAST/NORMAL channel limits match discovered capabilities.

### Run / Stop recording

- [x] **[CODE]** First Scope Run creates the first independent Scope record.
- [x] **[CODE]** Run changes the record state STOPPED -> LIVE.
- [x] **[CODE]** Run seeds matching recent baseline history into the new Scope record.
- [x] **[CODE]** Run returns the frontend viewport to Follow Latest and the default 0.5 s span.
- [x] **[CODE]** LIVE record receives decoded samples from the shared acquisition sink.
- [x] **[CODE]** Recorded duration derives from stored Scope samples while a real record exists.
- [x] **[CODE]** Stop freezes the Scope record and captures the then-current lost-frame count.
- [x] **[CODE]** Scope Stop only changes acquisition demand; it does not call Motor Stop.
- [x] **[CODE]** Baseline stays LIVE after Scope Stop.
- [x] **[CODE]** Tuning record is independent from Scope Stop.
- [x] **[CODE]** Frozen Scope snapshot remains unchanged while later baseline/Tuning samples arrive.
- [x] **[CODE]** A subsequent Run allocates a new Scope record rather than appending to the frozen one.
- [ ] **[HW]** Confirm recorded duration visibly increases during real LIVE capture.
- [ ] **[HW]** Stop a real capture and verify waveform/history remain byte-for-byte visually stable while status-bar telemetry continues.
- [ ] **[HW]** Start a second Run and verify it is a new recording seeded from current baseline history.

### Viewer / interaction

- [x] **[CODE]** Current viewer is one ECharts implementation shared with Control Tuning; no second active Scope renderer exists.
- [x] **[CODE]** Each selected channel owns an independent Y axis with physical Scale/div and Y Pos.
- [x] **[CODE]** Only the active channel's Y-axis labels are shown.
- [x] **[CODE]** Auto vertical scaling derives min/max from currently displayed captured data and changes presentation only.
- [x] **[CODE]** Horizontal zoom/pan is delegated to ECharts `dataZoom`.
- [x] **[CODE]** A user ECharts dataZoom event updates `viewRange`, leaves Follow Latest and requests the corresponding history window/offset.
- [x] **[CODE]** Latest keeps the current span and moves the view back to newest data.
- [x] **[CODE]** Double-click is wired to Latest through the ECharts ZRender surface.
- [x] **[CODE]** Page snapshot refresh is debounced for view changes and periodic only through the shared scheduler.
- [x] **[CODE]** Host downsampling preserves bucket extrema before sending points to ECharts; the chart itself does not enable a second lossy sampler.
- [x] **[CODE]** Lost-frame count comes from per-group sequence tracking and is frozen with a stopped Scope record.
- [ ] **[IMPL]** X1/X2 time cursors and Δt / Hz measurement.
- [ ] **[IMPL]** Per-channel cursor sample values / ΔY described by the Scope design.
- [ ] **[IMPL]** Persist and restore Scope view settings across application restart.
- [ ] **[HW/WEBVIEW]** Verify mouse-wheel/trackpad zoom behavior on the actual desktop WebView.
- [ ] **[HW/WEBVIEW]** Verify drag pan, Latest and double-click interactions with long retained real history.
- [ ] **[HW]** Verify lost-frame reporting against a deliberately stressed real stream.

### Automated guard status

- [x] **[CODE]** Low-level Plot Config/Start/Stop and FAST decode paths have scripted-transport tests.
- [x] **[CODE]** Plot capability pagination/discovery has a scripted-transport test.
- [x] **[CODE]** Shared baseline/Scope/Tuning record ownership has a scripted-transport test.
- [x] **[CODE]** Baseline preview, Run, Stop and visibility-only frontend behavior have Node harness tests.
- [x] **[CODE]** View-window/Latest/stale-snapshot handling has focused Node tests.
- [x] **[CODE]** Added a hot-reconfigure pending Config_ID promotion regression in this pass.
- [ ] **[TEST RUN]** Execute the current Rust/Node Scope tests after these changes.

### Exit criteria

- [ ] **Wired — blocked by missing normative cursor and view-persistence implementation.**
- [ ] **Verified — requires current real-device/WebView pass.**
- [ ] **Regression Guarded — coverage exists/improved, but current-pass tests have not been executed.**

---

## 04 Motor / Identification

**Primary owner:** ParameterService, MotorActionService, Identification semantic Application API  
**Shared consumers:** Motor page, Parameters, Problems, Automation, global motor controls  
**Normative references:** `MOTOR_PAGE.md`, `APPLICATION_PREFLIGHT.md`, `UI_INTERACTION_RULES.md`

### Code review pass — 2026-09-28

Three integration defects were found and corrected at shared owners rather than
patched locally in the Motor page:

1. **Compiled Default metadata was exported but discarded by NMIXX.**  
   Firmware HostSchema already emitted motor `default` values. HostSchema/
   Tauri/TS metadata now preserve that field and Motor Default cells consume it.

2. **Commissioning preflight did not surface blocking Protection state.**  
   Identification/Phase Search now explicitly read `PARAM_EVENT_ERROR` and
   `PARAM_EVENT_TRIP`. Non-zero masks produce structured Protection issues that
   point to Events before the workflow attempts Enable. Warning/Report remain
   non-blocking at this layer.

3. **Shared Identification Apply could apply the wrong historical result.**  
   Firmware `Identification_Apply()` applies according to internal current
   `Ident_Mode`; historical `*_VALID` flags do not select the Apply type.
   Application now owns a session-scoped Apply candidate created only by a
   successful finite identification completion with a valid result. GUI/Automation
   use an explicit kind match. Existing no-argument
   `ApplicationSession::identification_apply()` remains API-compatible and safely
   applies only the current candidate.

The Motor page may reconstruct stable **Applied** presentation from
`Active ~= Identified`, but it only offers **Apply** when Application says that
identification type is the current candidate. Older valid results remain visible
without an unsafe Apply button.

### Motor parameters

- [x] **[CODE]** Motor model rows consume the shared Parameter store/editor.
- [x] **[CODE]** Pole pairs/Rs/Ld/Lq/Flux/J/B use common Enter/Esc/blur RAM-write semantics.
- [x] **[CODE]** Motor model writes use generated HostSchema dependent readback rather than page-owned Control/Limits refresh lists.
- [x] **[CODE]** Generic Parameters and Motor views therefore converge on the same committed values.
- [x] **[CODE]** RAM-modified highlighting now applies only when firmware metadata marks the Motor Parameter persistent.
- [x] **[CODE]** HostSchema parses firmware compiled `default` values and Motor Default cells display that metadata.
- [ ] **[HW]** Compare Motor Default/Active values against the actual generated HostSchema/controller configuration.
- [ ] **[HW]** Edit each Motor model field and confirm Parameters plus affected Control/Limits values update to device readback.

### Common identification start / readiness

- [x] **[CODE]** GUI starts identification only through the semantic Application API.
- [x] **[CODE]** Application repeats authoritative preflight inside `MotorActionService::identification_start`; GUI does not own readiness rules.
- [x] **[CODE]** Preflight reads required current/speed/model/setup Parameters from the device through ParameterService.
- [x] **[CODE]** Preflight now blocks non-zero Protection Error/Trip with a structured Events/Protection issue.
- [x] **[CODE]** Application rejects identification start while Motor State is RUN.
- [x] **[CODE]** DISABLED start without authorization returns `RequiresEnable`; GUI asks for explicit user confirmation before retrying with `allowEnable=true`.
- [x] **[CODE]** Mode/Disable/Enable sequencing needed by firmware IDENT mode is contained in MotorActionService, not reproduced by the page.
- [x] **[CODE]** MotorGate/workflow checkpoint fences can cancel a pending start before later Action dispatch.
- [x] **[CODE]** Concurrent active Tuning blocks identification through the shared Application runtime.
- [x] **[CODE]** Identification start capability comes from HostSchema Actions.
- [ ] **[HW]** Verify blocked preflight reasons with missing/invalid prerequisites and active Protection fault.
- [ ] **[HW]** Verify explicit Enable confirmation path from DISABLED.
- [ ] **[HW]** Verify start from ENABLED/non-IDENT mode performs the intended safe mode transition.

### Rs/Ls

- [x] **[CODE]** Rs/Ls preflight requires positive current limit.
- [x] **[CODE]** Rs/Ls start maps to semantic `IdentificationKind::RsLs` / firmware Rs/Ls Action.
- [x] **[CODE]** Page tracks the finite Action handle and completion rather than treating accept response as completion.
- [x] **[CODE]** Application refreshes shared Parameters before completion reaches page subscribers.
- [x] **[CODE]** Rs/Ls VALID/result Parameters drive the Identified Rs/Ls display.
- [x] **[CODE]** Successful completion with VALID=1 creates the Rs/Ls Application Apply candidate.
- [x] **[CODE]** Rs/Ls Apply refreshes Active Rs/Ld/Lq through the immediate Action readback path.
- [ ] **[HW]** Run Rs/Ls to completion and compare displayed result with firmware/tool result.
- [ ] **[HW]** Apply Rs/Ls and verify Active Rs/Ld/Lq match the identified values.
- [ ] **[HW]** Stop during Rs/Ls and verify the shared motor Stop path terminates the workflow cleanly.

### Flux

- [x] **[CODE]** Flux preflight requires current/speed limits, pole pairs, active Rs/Ld/Lq and I/F startup current.
- [x] **[CODE]** Host preflight rejects I/F startup current above the configured current limit.
- [x] **[CODE]** Flux start/completion/result VALID path shares the same finite Action machinery as Rs/Ls.
- [x] **[CODE]** Successful Flux completion creates only the Flux Apply candidate, replacing eligibility of older identification types.
- [x] **[CODE]** Flux Apply refreshes Active Flux through shared Parameter state.
- [ ] **[HW]** Run Flux to completion with a known motor and verify result/VALID/failure reporting.
- [ ] **[HW]** Apply Flux and verify Active Flux updates exactly.
- [ ] **[HW]** Exercise Flux failure/Stop and confirm motor/application remain recoverable.

### J/B

- [x] **[CODE]** J/B capability comes from firmware Action exposure.
- [x] **[CODE]** J/B preflight requires current/speed limits, motor model including active Flux, and J/B excitation ratio/frequency.
- [x] **[CODE]** J/B setup fields use the shared Parameter editor.
- [x] **[CODE]** J/B completion/result VALID path shares the common Action/readback machinery.
- [x] **[CODE]** Successful J/B completion creates only the J/B Apply candidate.
- [x] **[CODE]** J/B Apply refreshes Active J/B through shared Parameter state.
- [ ] **[HW]** Run J/B to completion and verify identified J/B plus VALID/failure behavior.
- [ ] **[HW]** Apply J/B and verify Active J/B updates exactly.
- [ ] **[HW]** Cancel/Stop J/B and confirm no abandoned motor motion/workflow remains.

### Apply / stable presentation

- [x] **[CODE]** Historical `*_VALID` flags alone no longer make an Apply button available.
- [x] **[CODE]** Application records Apply candidate only after successful Action completion and valid result readback.
- [x] **[CODE]** Explicit-kind GUI/Automation Apply must match the current Application candidate.
- [x] **[CODE]** Existing no-kind Application/Tauri Apply remains backward-compatible but resolves only the current candidate.
- [x] **[CODE]** Apply candidate is cleared once Apply is dispatched, including uncertain transport failure.
- [x] **[CODE]** Page recreation queries Application candidate rather than reconstructing Apply eligibility from historical VALID flags.
- [x] **[CODE]** Page recreation can independently derive stable Applied presentation when Active values numerically match the valid identified result.
- [x] **[CODE]** Identified values remain visible even when an older result is no longer Apply-eligible.
- [ ] **[HW]** Produce multiple valid identification groups, then confirm only the most recently successful group offers Apply.
- [ ] **[HW]** Navigate away/back before Apply and confirm the correct candidate remains available.
- [ ] **[HW]** Reconnect after an old valid result and confirm the Host does not guess an unsafe Apply candidate.
- [ ] **[HW]** Manually edit an Applied Active parameter and confirm the page no longer implies that unchanged identified result is currently applied.

### Global Stop / failure feedback

- [x] **[CODE]** Motor page does not own an Identification Abort button/path.
- [x] **[CODE]** Global Stop remains connection-scoped and invokes `ApplicationSession::motor_stop`.
- [x] **[CODE]** Motor Stop revokes pending workflow starts through MotorGate before dispatch.
- [x] **[CODE]** Failed finite completion is surfaced in the corresponding identification row.
- [x] **[CODE]** Firmware failure reason Parameter is available for failed valid-result interpretation.
- [ ] **[HW]** Verify Stop response latency and final Motor State for each identification type.
- [ ] **[HW]** Verify a firmware identification failure appears once with useful status/reason and permits rerun.

### Automated guard status

- [x] **[CODE]** Architecture guard enforces semantic Motor/Encoder APIs rather than raw Action discovery.
- [x] **[CODE]** Added guard that Identification Apply is Application-candidate/kind scoped.
- [x] **[CODE]** Added guard that commissioning preflight consumes blocking Protection masks.
- [x] **[CODE]** HostSchema unit coverage includes compiled Parameter default metadata.
- [ ] **[TEST RUN]** Execute current Rust/frontend tests after these changes.
- [ ] **[TEST GAP]** Add scripted Application/MotorAction integration coverage for identification start/completion/candidate/apply sequencing if failures appear during real-device verification.

### Exit criteria

- [x] **Wired — code-reviewed after shared Apply/preflight/default fixes.**
- [ ] **Verified — requires current real-device pass.**
- [ ] **Regression Guarded — static/unit guards added, but current-pass tests have not been executed.**

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
