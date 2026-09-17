# UI Interaction Rules

## Purpose

This document defines interaction rules shared by NMIXX Motor Studio pages. These rules belong to the application workbench rather than to any one Motor, Encoder, Control, Tuning or Analysis page.

The goal is to keep state-changing controls predictable across the product while preserving the architectural boundary between global motor state, device-session management and domain-specific operations.

## Global status versus page actions

The application shell owns information and actions whose meaning does not depend on the current page.

Persistent global status should include, when available:

- device connection state;
- motor state (`DISABLED`, `ENABLED`, `RUN`, faulted states as applicable);
- compact live motor feedback intended to remain visible across workflows, such as position, speed and current.

Domain pages own actions whose meaning depends on that workflow, for example identification, phase search, homing, motion experiments, Scope capture, FFT or Bode operations.

The key rule is:

```text
Global shell
    Enable / Disable
    Stop

Domain page
    Run / Start the domain-specific operation
```

`Run` is not a generic global operation because its meaning depends on the current workflow and command configuration.

## Global motor controls

`Enable / Disable` and `Stop` are separate persistent motor controls.

```text
DISABLED   [ Enable ]   [ Stop ]
ENABLED    [ Disable ]  [ Stop ]
RUN        [ Disable ]  [ Stop ]
```

The `Enable / Disable` control is one stable stateful button, not two separate buttons. Its label, icon and color show the action that will occur when it is pressed:

- when the motor is disabled, the button presents `Enable` with the enable visual treatment;
- when the motor is enabled or running, the same button presents `Disable` with the disable visual treatment.

The control must not rely on color alone. Text, icon and color change together so its action remains unambiguous across themes and for users who do not distinguish the colors reliably.

`Stop` remains a separate button because stopping motion and changing motor enable state are different operations. The Stop control keeps a stable location even when it is currently unavailable; when no stoppable operation is active it may be disabled/muted rather than removed.

The intended state flow is typically:

```text
DISABLED
    |
    | Enable
    v
ENABLED
    |
    | domain-specific Run
    v
RUN
    |
    | Stop
    v
ENABLED
```

`Disable` is not a substitute for `Stop`, and `Stop` is not a substitute for `Disable`.

## Stateful paired actions

When two actions are mutually exclusive transitions of the same resource, use one stateful button in a stable location rather than two adjacent Start/Stop-style buttons.

Examples include:

```text
Enable  <-> Disable
Start   <-> Stop
Record  <-> Stop recording
Start search <-> Abort search
```

Use this pattern when all of the following are true:

1. the two actions operate on the same task/resource;
2. only one side is meaningful at a time;
3. the user naturally expects to stop/close the operation from the same place where it was started;
4. the current state is known reliably enough to render the next action.

The button always describes the action that pressing it will perform, not merely the current state.

Do not mechanically merge unrelated actions merely because their names form a verbal pair. In particular, the global `Enable / Disable` button does not absorb the separate global `Stop` action, and domain-specific `Run` does not become a global toggle.

## Color semantics

Color reinforces state/action meaning but never carries it alone.

Initial product convention:

- `Enable`: green enable treatment;
- `Disable`: red disable treatment;
- `Stop`: visually distinct stop treatment with a stop-square symbol; it should not be confused with Disable even when both are safety-relevant actions.

Exact theme tokens may evolve. The semantic distinction must remain.

## Connection controls

Connection state is global; connection configuration and session lifecycle controls belong to the Connection domain.

The application shell may continuously show that a device is connected or disconnected and may provide navigation to the Connection page. It does not need to duplicate transport configuration or a full Connect/Disconnect control group on every page.

The Connection page owns:

- transport selection;
- endpoint/device selection;
- transport-specific configuration;
- Host Schema selection while that remains an explicit development setting;
- Connect;
- Disconnect;
- device/session capability information.

`Disconnect` is not a motor Stop operation. Device-session teardown and motor safety behavior must remain separate concepts in both UI and Application API.

## Application API boundary

The shell and pages call semantic Application API operations. They do not assemble low-level Parameter/Action sequences themselves.

Global operations should have stable semantic entries such as:

```text
motor.enable
motor.disable
motor.stop
```

Domain operations remain explicit, for example:

```text
identification.start(Flux)
phaseSearch.start()
tuning.runExperiment(...)
scope.start()
```

This keeps GUI, CLI and automation aligned while allowing the GUI to apply the interaction rules defined here.
