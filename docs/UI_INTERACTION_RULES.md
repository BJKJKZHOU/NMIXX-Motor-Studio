# UI Interaction Rules

## Purpose

This document defines interaction rules shared by NMIXX Motor Studio pages. These rules belong to the application workbench rather than to any one Motor, Encoder, Control, Tuning or Analysis page.

The goal is to keep state-changing controls predictable across the product while preserving the architectural boundary between global motor state, device-session management and domain-specific operations.

## Workbench information hierarchy

The application shell separates actions, workflow content and persistent status:

```text
Top toolbar
    global motor actions
    global problem summary

Current page
    domain-specific configuration and actions

Bottom status bar
    connection/device status
    live Current / Speed / Position
```

Do not duplicate the same information in all three areas.

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

## Top toolbar problem indicator

The far-right area of the top toolbar contains the global Problems indicator.

It is a summary only. It must not become a miniature diagnostic panel.

The indicator shows:

- the severity of the most important currently active problem;
- a short broad-domain summary for that most important problem, such as `Encoder problem`, `Motor problem` or `Identification problem`;
- a badge count equal to the total number of currently active problems.

Example:

```text
[ Disable ] [ Stop ]        [ Error · Encoder problem  3 ]
```

When several active problems exist, the text still represents only the single highest-priority problem. The badge communicates the total count. Full per-problem information belongs on the Events / Problems page.

Priority is determined first by severity, then by application-defined importance within the same severity. The top bar must not rotate through several messages or concatenate long diagnostic text.

Hover may show only a short summary such as:

```text
3 active problems
Highest: Encoder problem
```

Detailed cause, effect, recovery instructions, error codes and timestamps remain on the Events / Problems page.

Clicking the indicator opens the Events / Problems page, normally focused on Active problems.

## Active problem refresh and resolution

The Problems indicator provides a direct refresh/recheck operation.

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

Left side:

- connection/device status.

Right side, right-aligned and in this stable order:

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
- problem severity: distinct Info / Warning / Error-or-Fault treatment while always retaining icon/text semantics.

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
