# UI Interaction Rules

## Purpose

This document defines interaction rules shared by NMIXX Motor Studio pages. These rules belong to the application workbench rather than to any one Motor, Encoder, Control, Tuning or Analysis page.

The goal is to keep state-changing controls predictable across the product while preserving the architectural boundary between global motor state, device-session management and domain-specific operations.

## Workbench information hierarchy

The application shell separates actions, workflow content and persistent status:

```text
Top toolbar
    global motor actions
    global parameter actions
    compact Problems indicator

Current page
    domain-specific configuration and actions

Bottom status bar
    connection/device identity
    live Current / Speed / Position
```

Do not duplicate the same information in all three areas.

The intended top-toolbar composition is:

```text
[ Enable / Disable ] [ Stop ] [ Read ] [ Save ]                         [ ⚠ 3 ]
```

The action group is aligned to the left edge of the main content area and must not extend over the left Activity Bar. The Problems indicator is aligned to the far right.

## Human-facing labels

HostSchema `label` is the user-facing name for Parameters and Actions. Business pages, notifications, validation errors and accessibility text must use the label rather than firmware macro/symbol names.

`symbol` remains the stable programmatic key used by Application/GUI code to look up metadata and actions. It may be shown deliberately in expert/debug surfaces such as the generic Parameters table, logs or diagnostics, but it is not a fallback UI label.

If expected metadata is unavailable, normal business UI should show an unavailable/missing state or an explicit human fallback defined by that workflow. It must not expose `PARAM_*`, `ACTION_*` or C variable-style names merely because schema lookup failed.

## Global motor controls

`Enable / Disable` and `Stop` are separate persistent motor controls in the top toolbar.

```text
DISABLED   [ Enable ]   [ Stop ]
ENABLED    [ Disable ]  [ Stop ]
RUN        [ Disable ]  [ Stop ]
```

The `Enable / Disable` control is one stable stateful button, not two separate buttons. Its label, icon and color show the action that will occur when it is pressed:

- when the motor is disabled, the button presents `Enable` with the enable visual treatment;
- when the motor is enabled or running, the same button presents `Disable` with the disable visual treatment.

The control must not rely on color alone. Text, icon and color change together so its action remains unambiguous across themes and for users who do not distinguish the colors reliably.

`Stop` remains a separate button because stopping motion and changing motor enable state are different operations. The Stop control keeps a stable location even when it is currently unavailable; when no stoppable operation is active it is disabled/muted rather than removed.

`Run` is not a generic global action. Run/Start belongs to the domain that defines what operation will execute.

## Global parameter controls

`Read` and `Save` are global configuration actions in the top toolbar.

```text
[ Read ] [ Save ]
```

`Read` means refresh the host from the device's current active RAM parameter values. It does not reload Flash into RAM.

A successful device connection automatically performs one initial Read so the Application Parameter service is populated before normal workflows use the values. The explicit Read button remains available to resynchronize after external changes, Actions or debugging operations.

Normal parameter edits made from Motor, Encoder, Limits, Control, Parameters or other pages update the device's active RAM state only. They do not implicitly persist the change to Flash.

`Save` means explicitly persist the current firmware-defined persistent configuration from RAM to non-volatile storage. Persistence ownership stays in firmware; the host must not implement storage by maintaining its own list of values and replaying them as a pseudo-Flash format.

The intended semantic direction is:

```text
Read
    Device RAM -> ParameterService / GUI

normal edit
    GUI -> Device RAM

Save
    Device RAM persistent configuration -> Flash/NVS
```

A future Reload Saved Configuration operation, if added, is a distinct destructive action because it would overwrite current RAM changes from Flash. It must not be conflated with Read.

### RAM-modified presentation

After a successful Parameter write, the control that presents that Parameter keeps a low-intensity modified background while its current RAM value differs from the last successfully persisted configuration.

This state is session-wide, not page-local:

- navigating away from a page and returning must not clear it;
- refreshing a page from the shared Parameter cache must not clear it;
- the generic Parameters page and business pages must reflect the same modified state for the same Parameter;
- writing the value back to the persisted baseline removes the modified state for that Parameter;
- a successful global `Save` establishes the current RAM values as the new persisted baseline and clears the remaining modified indications;
- a failed or unavailable Save does not clear them.

Do not confuse this with an input draft. A draft is text that has not yet been committed with Enter; the RAM-modified state begins only after the write succeeds.

## Stateful paired actions

When two actions are mutually exclusive transitions of the same resource, use one stateful button in a stable location rather than two adjacent Start/Stop-style buttons.

Examples include:

```text
Enable <-> Disable
Start <-> Stop
Record <-> Stop recording
Start search <-> Abort search
```

Use this pattern when the actions operate on the same task/resource and only one side is meaningful at a time. The button always describes the action that pressing it will perform.

Do not merge unrelated actions merely because their names form a verbal pair. In particular, the global `Enable / Disable` button does not absorb the separate global `Stop` action, and domain-specific `Run` does not become a global toggle.

## Top toolbar Problems indicator

The far-right area of the top toolbar contains a compact Problems indicator.

Its persistent collapsed form contains only:

```text
[ severity icon ] [ active problem count ]
```

Example:

```text
⚠ 3
```

The badge count equals the total number of currently active problems. The icon reflects the highest active severity. No problem text is shown persistently in the toolbar.

Clicking the compact indicator opens a small summary popover. The popover shows only the single highest-priority active problem as a short message, for example:

```text
Encoder problem
```

It may also provide a compact Refresh/Recheck control.

Clicking the short problem message navigates to the Events / Problems page, normally focused on the corresponding active problem. The popover must not attempt to show full cause, effect, recovery guidance, codes or timestamps.

Priority is determined first by severity, then by application-defined importance within the same severity. The top bar must not rotate through several messages or concatenate multiple problem summaries.

## Active problem refresh and resolution

The Problems summary may expose a direct Refresh/Recheck action.

Refresh means:

```text
re-read / re-evaluate current problem conditions
        |
        +-- condition still present -> keep Active
        |
        +-- condition passes now -> remove from Active
```

A successful recheck clears the problem from the active problem set and therefore reduces the top-bar badge immediately.

This does **not** delete the event history. The corresponding history record remains and is marked/resolved according to the actual lifecycle.

The following operations are distinct:

```text
Refresh / recheck current problems
    -> may resolve/remove Active items

Clear device fault
    -> optional device operation when firmware exposes such semantics

Clear history
    -> explicitly deletes retained history records
```

Viewing, clicking, refreshing or resolving a problem must not silently erase its historical record.

## Events / Problems page presentation

The detailed page uses a three-column problem-oriented layout for Active items:

```text
Level / Hint        Detailed description                 Quick action
--------------------------------------------------------------------------
Error · Encoder     Why it happened, effect and          [ Open Encoder ]
                    what normally needs checking

Warning · Limits    Why the warning exists and what      [ Open Limits ]
                    configuration is missing
```

The left column is concise: severity plus a short hint/domain.

The middle column carries the useful diagnostic explanation:

- what happened;
- why this problem can occur;
- what it affects;
- what the user should normally inspect or change.

The right column provides a shortcut to the page where the issue is normally resolved. Navigation is advisory; the problem itself remains owned by the Application/device state, not by the destination page.

The Events / Problems page also retains History. Resolved problems remain in history until the user explicitly clears history.

Detailed lifecycle and page semantics are defined in `EVENTS_PAGE.md`.

## Bottom status bar

The bottom status bar contains persistent low-interruption runtime information.

### Left side

The left side shows **connection status and device identity only**.

Typical presentation:

```text
● AxDr_L
```

Disconnected:

```text
○ Disconnected
```

Do not repeat motor enable/run state, control mode, faults/warnings, unsaved-parameter state, Scope state or acquisition statistics here. Those concepts already have dedicated UI ownership elsewhere.

The status bar should present the identity of the connected device rather than detailed transport configuration. Endpoint, serial port, CAN interface/node, baud/bitrate, Host Schema and other connection details belong to the Connection page or an optional hover tooltip.

If future multi-device or CAN-node workflows make disambiguation necessary, the compact identity may expand, for example:

```text
● NMIXX · Node 3
```

but single-device sessions should remain visually minimal.

### Right side

The right side is right-aligned and keeps this stable order:

```text
Current    Speed    Position
```

The intended meanings are:

- **Current**: actual q-axis current `Iq` unless a future product decision explicitly changes the displayed quantity;
- **Speed**: mechanical speed;
- **Position**: user mechanical position, retaining the multi-turn `Turn + Theta` representation when appropriate.

Page-specific acquisition information such as Scope state, FAST sample rate, selected channels or stream-loss counters belongs to Analysis, not to the global status bar.

## Color semantics

Color reinforces state/action meaning but never carries it alone.

Initial product convention:

- `Enable`: green enable treatment;
- `Disable`: red disable treatment;
- `Stop`: visually distinct stop treatment with a stop-square symbol;
- problem severity: distinct Info / Warning / Error-or-Fault treatment while always retaining icon semantics.

Exact theme tokens may evolve. Semantic distinction must remain.

## Connection controls

Connection state is global; connection configuration and session lifecycle controls belong to the Connection domain.

The bottom status bar continuously shows connected/disconnected state and device identity. It does not duplicate the full connection controls.

The Connection page owns:

- transport selection;
- endpoint/device selection;
- transport-specific configuration;
- Host Schema selection while that remains an explicit development setting;
- Connect;
- Disconnect;
- device/session capability information.

`Disconnect` is not a motor Stop operation. Device-session teardown and motor safety behavior remain separate concepts in both UI and Application API.

## Application API boundary

The shell and pages call semantic Application API operations. They do not assemble low-level Parameter/Action sequences themselves.

Global operations should have stable semantic entries such as:

```text
motor.enable
motor.disable
motor.stop
parameters.refresh
config.save
problems.refresh
```

Domain operations remain explicit, for example:

```text
identification.start(Flux)
phaseSearch.start()
tuning.runExperiment(...)
scope.start()
```

Problems should be structured enough that the GUI can render severity, domain, explanation and a suggested destination without parsing opaque error strings.
