# Control Tuning Page

## Purpose

Control Tuning is a servo closed-loop tuning page. It is not the same workflow as the generic Plot/Scope page, and it is not the control-structure construction page.

Control Architecture is a parent domain with Current Loop, Speed Loop and Position Loop child pages. Those loop pages own controller/observer selection and structural control-path configuration such as filters, feedback selection, feedforward and other algorithm-specific blocks. Control Tuning remains a separate top-level workflow: it assumes that structure already exists and concentrates the parameters engineers adjust repeatedly while observing dynamic response.

The page binds three things into one repeatable experiment:

1. the common / primary controller and observer tuning parameters;
2. one motion command;
3. one finite waveform capture synchronized to that motion.

The intended workflow is:

> explicitly Enable from the global motor controls -> select mode -> adjust parameters -> configure one motion -> Run -> inspect the complete response -> adjust again.

The GUI operates through the Application API. It must not assemble low-level protocol transactions directly.

## Global motor controls are not owned by this page

Connection state and motor state are global application context, but their controls have different ownership.

- Connection configuration, Connect and Disconnect belong to the Connection page.
- Motor `Enable / Disable` and `Stop` are persistent global motor controls owned by the application shell.
- The Control Tuning page owns only the domain-specific experiment `Run` action.

The shared interaction rules are defined in `UI_INTERACTION_RULES.md`.

The persistent motor controls use two stable control positions:

```text
Motor state        Enable / Disable              Stop
---------------------------------------------------------
DISABLED           [ Enable  ]                   [ Stop ] disabled
ENABLED            [ Disable ]                   [ Stop ] disabled
RUN                [ Disable ]                   [ Stop ] active
```

`Enable / Disable` is one stateful button. Text, icon and color change together to show the action that pressing it will perform. `Stop` remains a separate persistent button because stopping motion is not the same operation as disabling the motor.

Control Tuning must never automatically Enable or Disable the motor as a side effect of `Run`, page entry, page exit, parameter editing or capture control.

`Run` is only valid when the motor is already `ENABLED`. If it is not enabled, the page reports that the user must explicitly Enable the motor from the global control area; it must not enable on the user's behalf.

## Layout

The first version uses three functional regions below the persistent application shell:

```text
Application shell
Device: AxDr_L · Connected       Motor: ENABLED   [ Disable ] [ Stop ]

+---------------------------------------------------------------+
| Control Tuning                                                |
+-----------------------------------------+---------------------+
|                                         | Current Loop        |
|                                         | Source [Bandwidth v]|
|                                         | Bandwidth [1000] Hz |
|                                         | Id Kp [0.0178]      |
|              Experiment                 | Id Ki [532.3]       |
|              Waveform                   | Iq Kp [...]         |
|                                         | Iq Ki [...]         |
|                                         |                     |
|                                         | Speed Loop          |
|                                         | Source [Bandwidth v]|
|                                         | Bandwidth [50] Hz   |
|                                         | Kp [...]   Ki [...] |
|                                         |                     |
|                                         | Position Loop       |
|                                         | Kp         [5.0]    |
+-----------------------------------------+                     |
| Motion Command                          | Mechanical Observer  |
| Mode [ Position v ]                     | Bandwidth [100] Hz  |
| Target [...]  Speed [...]               |                     |
| Acc [...]     Dec [...]                 |                     |
|                         [ Run ]          |                     |
+-----------------------------------------+---------------------+
```

The left side is the experiment waveform area. The lower-left area defines the motion for the next experiment. The right side contains the compact primary tuning set and the actual controller gains used by firmware. Current and speed loops expose both Bandwidth and Manual gain ownership; the active source is visible and editable.

The page skeleton is stable. Waveform-related configuration must remain inside the existing Experiment Waveform panel; it must not move Motion Command or the right-side tuning parameter column.

Inside the Experiment Waveform panel, three browser-style sub-tabs share the same bounded content area:

- **Waveform** — the captured multi-channel time plot;
- **Channels** — capture channel/rate selection;
- **Scale** — per-channel display multiplier.

Switching tabs replaces the waveform content area rather than expanding an extra settings panel above the waveform. This preserves the existing page proportions.

The page deliberately does not reproduce the complete control diagram. Structural configuration such as controller type, filter mode/frequency, feedback source, feedforward and algorithm-specific block options belongs to the corresponding Current / Speed / Position child page under Control Architecture even when those values ultimately affect the same loop.

The page does not duplicate Connect/Disconnect controls or motor Enable/Disable/Stop controls inside its own content area.

### Shared Motion model, tuning-specific presentation

The Motion command area is not a separate tuning-only command model. It is a Control Tuning-specific UI projection over the same Application-level Motion model used by the full Motion page.

The Control Tuning page intentionally keeps its compact workflow-oriented presentation: Mode, Position command semantics, Repeat, Position/Speed target fields, Acc/Dec, Copy Accel to Decel, and Run/Stop. It does not have to reuse the full Motion page component or expose every advanced Motion setting.

Changing a field that is shown here updates the same Motion state seen by the full Motion page, and vice versa. Run/Stop call the same Motion Application API; there is no second page-local command copy.

Control Tuning normally operates as a repeated ENABLED-state workflow: tune parameters -> Run -> inspect response -> tune again -> Run. Run does not automatically Enable/Disable the motor and does not require a confirmation dialog. When the motor is not ENABLED, Run is simply unavailable with a concise reason such as "Enable motor first".

The finite experiment waveform/orchestration remains a separate Control Tuning responsibility. Sharing the Motion model does not mean the bounded tuning-capture workflow has been implemented by the GUI itself.

## Tuning parameter semantics

Bandwidth is a design entry; Kp/Ki are the actual controller parameters. Current and speed loops also expose a tuning source that defines who owns the actual gains when motor-model parameters change.

These Parameters may also appear inside their corresponding blocks on Control Architecture. That is intentional: the pages are two views over the same shared Parameter state, not two copies. A write from either page must immediately be reflected by the other view, including the shared RAM-modified / not-yet-saved presentation.

Control Tuning should remain selective. A Parameter belongs here when engineers are expected to change it repeatedly while running response experiments. Structural parameters that are changed occasionally belong only to Control Architecture.

### Current Loop

Writable:

- current-loop bandwidth, Hz;
- tuning source: `Bandwidth` / `Manual`;
- Id Kp / Ki;
- Iq Kp / Ki.

Semantics:

- writing Bandwidth switches the source to `Bandwidth` and firmware recalculates Id/Iq gains from active `Rs/Ld/Lq`;
- writing any Id/Iq Kp/Ki switches the source to `Manual`;
- when the source is `Bandwidth`, later motor-parameter changes may refresh the gains;
- when the source is `Manual`, later motor-parameter changes must preserve the user-written gains;
- explicitly selecting `Bandwidth` recalculates gains from the current bandwidth and motor model, even when the bandwidth numeric value itself did not change.

The page keeps Bandwidth and all four actual gains visible at the same time. Direct gain tuning is not hidden behind a separate Advanced dialog.

### Speed Loop

Writable:

- speed-loop bandwidth, Hz;
- tuning source: `Bandwidth` / `Manual`;
- Kp;
- Ki.

The same ownership rule applies: Bandwidth writes select automatic model-based design; direct Kp/Ki writes select Manual ownership.

### Position Loop

Writable:

- position-loop Kp.

The first implementation does not add unnecessary additional position-loop parameters.

### Mechanical Observer

Writable:

- mechanical ESO bandwidth, Hz.

The observer states / gains may be exposed as diagnostics when required, but the first UI does not require users to edit `L1/L2/L3` directly.

## Parameter editing and motor state

Tuning parameters may be edited while the motor is either `DISABLED` or `ENABLED`.

The first implementation does not permit tuning writes while the motor is `RUN`.

This supports the intended workflow:

```text
DISABLED
  -> parameters may be edited
  -> user explicitly presses global Enable
ENABLED
  -> parameters may still be edited
  -> user presses page-local Run for an experiment
RUN
  -> tuning controls locked / firmware rejects tuning writes
  -> global Stop is available
  -> experiment finishes or Stop returns motor to ENABLED
ENABLED
  -> user adjusts parameters and runs the next experiment
```

Parameter editing must not cause implicit Enable, Disable, Run or Stop actions.

## Run behavior

Parameter editing follows the shared application rule:

```text
typing -> GUI draft only
Enter  -> write Device RAM and read back the effective value
Esc    -> discard draft
blur   -> no commit
```

`Run` never commits pending tuning drafts. If any tuning field still differs from the last effective Device RAM value, the page refuses to start the experiment and asks the user to commit or discard the edit first.

When the user presses `Run`, the Application API performs one complete experiment using the already-effective parameter set and assumes the user has explicitly put the motor in `ENABLED` state:

```text
verify no uncommitted tuning draft
        |
        +-- no --> show "Commit or discard edited values first" and stop
        |
       yes
        v
verify motor state == ENABLED
        |
        +-- no --> show "Enable motor first" and stop
        |
       yes
        v
read back effective tuning values
        |
        v
configure motion command
        |
        v
start experiment capture
        |
        v
pre-motion capture
        |
        v
Run
        |
        v
execute motion
        |
        v
finish condition
        |
        +-- Position, Repeat off -> trajectory time + settling -> controlled Stop
        |
        +-- Speed / Sensorless Speed / Torque / Repeat -> user Stop
        |
        +-- persistent global Stop -> same experiment stop path
        v
wait until motor leaves RUN
        |
        v
post-motion capture
        |
        v
stop capture and retain waveform
```

There is deliberately no implicit `Enable` or `Disable` in this sequence.

While an experiment is running, the tuning controls should be locked in the first implementation. Online gain editing can be added later only if there is a concrete need.

The page-local `Run` action is contextual: it means "run this configured tuning experiment". It must not be promoted to a generic global Run button.

## Global Stop during an experiment

The persistent global `Stop` control must remain available while a tuning experiment is in `RUN`.

Control Tuning also exposes a local `Stop` next to its page-local `Run` because that placement is convenient during repeated experiments. Both controls terminate the same active motion/task through the Application layer and return the motor to the appropriate non-running state, normally `ENABLED`.

The page-local Stop must not implement an independent device-side stop path. It is a second UI entry to the same Application stop operation.

A Stop should preserve a coherent experiment result/error state and retain the waveform collected up to the stop.

## Experiment capture

This page does not use an endless scrolling Plot session.

A capture is bounded by one tuning experiment rather than by a fixed duration for every Motion mode.

The initial lifecycle uses:

- pre-motion capture: 0.5 s;
- Position with Repeat disabled: run for the calculated trajectory duration, keep a short settling interval, then issue the normal controlled Stop;
- Speed / Sensorless Speed / Torque and repeated Position: continue until the user presses local or global Stop;
- post-stop capture: 0.75 s after the motor leaves RUN;
- one finite non-overwriting raw capture for the complete experiment, bounded by a 128 MiB RAM protection budget;
- a shorter live preview window while the experiment is running.

The post-stop interval is important because position/speed tuning must show the stop transient, residual vibration and settling. Control Tuning does not silently roll over or overwrite the beginning of an experiment. The Application converts the selected FAST/NORMAL channel rates into the maximum finite capture duration that fits the 128 MiB raw-sample budget. As the capture approaches that limit, it issues a controlled Stop early enough to preserve the post-capture interval, retains the samples already recorded, and reports that the recording limit was reached.

The waveform remains on screen after capture ends so the user can inspect the complete response before changing the next parameter set.

If the experiment is stopped early, the capture/task layer should still finish the record coherently rather than leaving the plotting service in an unrelated running state.

## Default waveform channels

Control Tuning provides a recommended channel set for each Motion mode, but the set is a default rather than a mandatory fixed group. Before Run, the user may add/remove any Plot-capable channel and choose FAST/NORMAL where the firmware supports both. The configured selection is locked while one experiment is active.

The recommended defaults always include the q-axis current command and feedback at the FAST rate:

- **Iq Ref** — FAST, device current-loop rate (20 kHz on the current AxDr_L firmware);
- **Iq** — FAST, same rate.

These two channels remain present for Position, Speed and Torque tuning so current-loop noise, high-frequency oscillation and the actuator effort demanded by outer loops remain visible.

Motion reference/feedback channels use NORMAL rate (1 kHz on the current AxDr_L firmware). The first default sets are:

### Position experiment

- Iq Ref — FAST;
- Iq — FAST;
- Position Ref — NORMAL;
- Position — NORMAL;
- ωm Ref — NORMAL;
- ωm — NORMAL.

`Position Ref` is the trajectory output actually used by position control, not the final Position Target command.

### Speed / Sensorless Speed experiment

- Iq Ref — FAST;
- Iq — FAST;
- ωm Ref — NORMAL;
- ωm — NORMAL.

`ωm Ref` is the ramp/trajectory reference actually used by speed control, not the raw target speed.

### Torque experiment

- Iq Ref — FAST;
- Iq — FAST.

Additional diagnostics such as encoder-difference speed, Mechanical ESO state or disturbance torque may be selected by the user without changing the default set.

FAST and NORMAL samples keep their native sample rates. The GUI does not upsample 1 kHz motion signals to 20 kHz. All captured tuning signals are aligned on one shared experiment time axis and rendered in one waveform area rather than separate Current / Speed / Position charts.

### Waveform interaction

The tuning waveform is intentionally simpler than the general Scope page.

It supports:

- one shared horizontal time axis;
- mouse-wheel Time/div changes;
- horizontal drag within the finite captured record;
- stable per-channel color identity within the capture;
- one positive display multiplier per selected channel.

The Scale tab edits the multiplier directly as `×N`. The rendering rule is exactly `display_y = raw_sample × multiplier`. With `×1`, the plotted curve is the original floating-point sample value without per-channel normalization or per-channel auto-ranging. All tuning curves share one display Y scale, so relative numeric magnitude is preserved unless the user explicitly changes a multiplier. The multiplier is display-only: raw capture values, exports and future measurements continue to use the original physical values. The tuning waveform does not expose Y offset, Y-position markers, cursors, trigger controls, Follow Latest, or the full Scope acquisition controls.

The intended workflow is to configure channels and display multipliers infrequently, then repeatedly change tuning parameters, Run the same motion, inspect the waveform, and tune again.

## Motion Command

The lower-left Motion Command area uses a compact two-column engineering form rather than a tall settings stack. Position mode should present command-mode options and the common motion values in one bounded area, for example:

```text
Mode [ Position v ]       ( ) Absolute   (*) Incremental   [x] Repeat

Position   [ ... ] turn   Max Speed [ ... ] rad/s
Accel      [ ... ] rad/s² Decel     [ ... ] rad/s²

[ ] Copy Accel to Decel                         [ Run ] [ Stop ]
```

The local `Stop` is intentionally present beside `Run` for direct operator access. It is not a second stop mechanism: both the local Control Tuning Stop and the persistent global Stop invoke the same Application-level motor/task stop semantics.

When stopped early, the experiment capture should be finalized coherently and the acquired waveform retained for inspection.

The form depends on the selected mode.

Position:

- target position;
- maximum speed;
- acceleration;
- deceleration.

Speed:

- target speed;
- acceleration;
- deceleration;
- optional hold duration.

Torque:

- target torque;
- optional hold duration.

The first implementation may support only the modes that have a complete firmware/Application API path. Unsupported fields must not be faked in the GUI.

## Difference from Plot / Scope

`Scope` and `Control Tuning` may reuse the same lower-level streaming/capture infrastructure, but they have different product behavior.

Scope:

- continuous observation or manually controlled capture;
- freely selected variables;
- open-ended/general-purpose debugging workflow.

Control Tuning:

- one finite capture per experiment;
- tuning parameters, motion command and waveform are bound together;
- capture starts before the motion and ends after settling;
- intended specifically for closed-loop tuning.

Start/Stop-style controls inside Analysis should follow the shared stateful-action rule when they operate on one mutually exclusive acquisition task.

## Boundary with Control Architecture

Typical Parameters shared by both pages:

- current-loop Bandwidth and Id/Iq Kp/Ki;
- speed-loop Bandwidth and Kp/Ki;
- position-loop Kp;
- mechanical ESO Bandwidth;
- current/speed tuning source when it is useful during tuning.

Typical Control-Architecture-only configuration:

- controller type;
- feedback source;
- observer selection;
- output/input filter enable, mode and frequency;
- notch-filter configuration;
- feedforward enable/type/gain;
- anti-windup mode;
- algorithm-specific structural options.

Do not expand Control Tuning into a second control-architecture editor merely because a new host-visible Parameter exists.

## Firmware contract required by this page

The firmware should expose these writable design parameters through the Parameter Core:

- Current-loop bandwidth;
- Speed-loop bandwidth;
- Position-loop Kp;
- Mechanical ESO bandwidth.

These tuning parameters must be writable in `DISABLED` and `ENABLED`, and rejected in `RUN`.

It should also expose these writable actual controller parameters:

- Id Kp / Ki;
- Iq Kp / Ki;
- Speed Kp / Ki.

Current and Speed tuning source parameters must also be exposed so GUI, CLI and automation can observe or explicitly select `Bandwidth` / `Manual`.

Observer diagnostic states and old encoder-difference speed can be added as read-only Plot channels when the tuning workflow needs them.

Motor parameters (`Rs/Ld/Lq/Flux/J/B/Pp`), limits and motion configuration remain separate parameters and are reused by this page rather than duplicated.
