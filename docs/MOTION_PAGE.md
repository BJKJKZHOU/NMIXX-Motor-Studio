# Motion page interaction design

> **Normative Motion interaction contract.** Implementation must conform to this
> behavior; current code is not the source of product truth. See `docs/README.md`.

## Purpose

Motion is the direct command workspace for an already commissioned motor. It owns
mode selection, command values, supported trajectory/profile Parameters, trajectory
preview and explicit Run/Stop controls.

It does not own motor Enable/Disable, controller tuning, Scope acquisition, encoder
calibration or operating-limit policy.

## Shared state ownership

Firmware-visible Motion values remain normal shared Parameters:

```text
PARAM_MOTOR_MODE
PARAM_TARGET_TORQUE
PARAM_TARGET_SPEED
PARAM_TARGET_POSITION
PARAM_MOTION_WM_MAX
PARAM_MOTION_WM_ACC
PARAM_MOTION_WM_DEC
...
```

Motion, Control Tuning and the generic Parameters page therefore present the same
committed values. There is no Motion-local copy of a firmware Parameter.

Host-only command semantics may exist only when they cannot be represented as a
single firmware Parameter. The current Position command type and incremental delta
are examples:

```text
positionCommand       Absolute | Incremental
incrementalDeltaTurn  host-owned delta
```

Changing those host-owned values must not silently write unrelated firmware
Parameters.

## Mode selection

Available modes come from HostSchema/Application capabilities. The GUI must not
invent modes that the connected firmware does not expose.

The current product surface may include:

- Torque;
- Speed;
- Position;
- Sensorless Speed when firmware exposes it.

Mode changes obey firmware state restrictions. With the current firmware, mode is
changed while the motor is DISABLED. Motion UI may disable the selector outside
that state; Application/firmware validation remains authoritative.

## Command semantics

### Torque

Torque uses the committed Torque target Parameter. Optional ramp controls appear
only when firmware exposes their Parameters.

### Speed

Speed uses the committed mechanical target-speed Parameter. Sensorless Speed may
show additional startup/handover Parameters only when firmware exposes them.

### Position

Position supports two command forms:

- **Absolute** — the committed exact typed Position target is the requested user
  mechanical position.
- **Incremental** — the host-owned delta is added to an authoritative current
  position at execution time, then the exact typed target is written before Run.

Incremental delta is not an uncommitted Parameter draft.

## Trajectory/profile capability

The page shows only executable trajectory/profile behavior advertised by the
connected firmware.

Current AxDr_L Position/Speed motion uses the trapezoidal/T profile represented by
`Wm_Max / Wm_Acc / Wm_Dec`. S-curve, filtered-profile or other controls must not
be presented as normal executable choices merely because a preview implementation
exists.

When future firmware exposes another profile through stable Parameters/capability
metadata, the page may add that option without creating a second Host-owned copy of
those values.

## Preview semantics

Trajectory preview is a read-only projection.

It must:

- use committed shared Parameter values, not editor drafts;
- never write the device;
- never perform periodic device reads solely to redraw the preview;
- respect the firmware-reported effective speed limit where that limit affects the
  requested trajectory;
- keep Speed preview independent of Position-only inputs;
- keep Torque preview independent of Position/Speed profile inputs.

For Absolute Position preview, the current display start position may come from the
latest 1 kHz RuntimeTelemetry Plot position. That value is suitable for display
projection but is not the exact typed Position Parameter.

Execution keeps a stricter boundary:

```text
Preview
    latest RuntimeTelemetry total-turn position is acceptable

Run
    read exact typed PARAM_RUN_POSITION from device when current position is needed
```

The f32 Plot position must never overwrite the exact typed Position Parameter.

## Run semantics

Run is explicit and domain-local.

- Run never implicitly Enables the motor.
- The motor must already be ENABLED.
- The selected firmware mode and command Parameters must already be committed.
- Any pending normal Parameter writes/host Motion-state updates are completed before
  Run is dispatched.
- One Run interaction sends one semantic Motion Run request.
- Device/runtime feedback remains authoritative for actual motion.

Run acceptance is not universal proof that motion has finished. Normal Speed,
Torque and current Position motion do not expose a generic finite
ACTION_COMPLETE contract in the current firmware.

## Stop semantics

Motion Stop is the same Application motor Stop semantic used by the workbench.

It is not a page-local Abort implementation.

For motion modes that use controlled deceleration, successful Stop acceptance does
not mean the motor instantly leaves RUN. Firmware may remain RUN until the
configured deceleration reaches zero. The Host must observe that state rather than
inventing a second completion meaning.

Disable remains distinct from Stop.

## Repeat — deferred product semantic

The Motion page contains a Repeat concept, but the execution contract has not yet
been formally selected.

Do **not** infer one of these behaviors from the current code:

- one Run automatically executes a forward and reverse pair;
- Repeat continuously alternates until Stop;
- each separate Run alternates the target.

The current AxDr_L generic Motor Run does not emit an ACTION_COMPLETE event for
normal Position motion, so an implementation that advances Repeat purely from
generic Action completion is not valid.

Until an explicit product decision defines Repeat completion and sequencing, Repeat
must not be considered Wired/Verified. Any future implementation belongs in the
Application Motion workflow and must remain cancellable by the shared Stop path.

## Relationship to Control Tuning

Motion and Control Tuning may expose the same committed Motion Parameters, but they
serve different workflows.

Motion is the general direct-command workspace. Control Tuning owns its bounded
experiment lifecycle and acquisition. Control Tuning must not create a second copy
of Motion Parameter state.

## Non-goals

Motion does not:

- auto-enable the motor;
- own global Stop semantics;
- replace Limits / Safety;
- infer unsupported firmware trajectories;
- use Plot RuntimeTelemetry as an exact control Position;
- create its own device transport or acquisition stream.
