# Motor Page

## Purpose

The Motor page is the human-facing motor-parameter and identification workspace. It organizes the motor model around the parameters an engineer needs to provide or identify, while continuing to use the shared `ParameterService` and Application Action API.

It is not a second parameter database and it does not own low-level identification algorithms.

## Parameter table

The page uses a compact row-oriented layout:

```text
Parameter        Default        Active        Identified        Action
-----------------------------------------------------------------------
Pole pairs       —              [16]          —                 —
Rs               —              [...]         Rs result         [Rs/Ls]
Ld               —              [...]         Ls result         —
Lq               —              [...]         Ls result         —
Flux             —              [...]         Flux result       [Flux]
J                —              [...]         J result          [J/B]
B                —              [...]         B result          —
```

The columns have different meanings:

- **Default**: firmware-compiled/default value when the Host schema eventually exposes it. Until then the GUI shows `—` rather than inventing a value.
- **Active**: the parameter currently used by firmware control/identification logic. Writable values use the shared Parameter API.
- **Identified**: the latest valid identification result for the corresponding identification group.
- **Action**: the semantic identification operation associated with that result.

The page must not create separate storage for Active or Identified values.

## Identification groups

The initial page groups motor identification into three user-facing Actions:

```text
Rs/Ls
Flux
J/B
```

Each Action may have an internal firmware state machine, observer, startup procedure or fitting process. Those internal phases are not exposed as editable Parameters and are not reproduced in the GUI.

The Application layer owns Action start/completion semantics and readiness/preflight policy. The page renders the resulting state.

## Identification state presentation

The Action cell represents one identification workflow in a stable location.

Typical presentation keeps the identification start control available in its normal location and renders workflow state beside it:

```text
idle        [ Rs/Ls ]
running     [ Rs/Ls ]  Running
success     [ Rs/Ls ]  [ Apply ]
applying    [ Rs/Ls ]  Applying
applied     [ Rs/Ls ]  Applied
failure     [ Rs/Ls ]  Failed
```

The start control is disabled only while the workflow is actively running/applying. After failure it becomes available again so the user can run the identification again directly.

A successful identification result is not automatically copied into the Active motor parameters. The latest valid result appears in the **Identified** column and an **Apply** action appears beside the identification control.

This distinction is deliberate:

```text
identify
    -> produce candidate result
    -> show Identified
    -> user explicitly Apply
    -> update Active motor parameters
```

The result remains inspectable before the user changes the active motor model.

The page should present running/success/failure/apply state next to the corresponding identification group rather than introducing a separate dashboard of identification status flags.

## Apply semantics

`Apply` is an Action, not a direct GUI copy between fields.

The current firmware exposes a shared identification apply action. The Application/UI associates the apply request with the identification result that is currently ready and refreshes the affected Active parameters after completion.

The initial mappings are:

```text
Rs/Ls result
    -> Rs
    -> Ld
    -> Lq

Flux result
    -> Flux

J/B result
    -> J
    -> B
```

If an Active value is edited manually after an identified result has been applied, the UI must not continue implying that the active value still represents the unchanged applied result.

Identification presentation must also survive page recreation. When the Motor page is entered, it derives the stable result state from device Parameters rather than assuming `idle`:

- result valid and Active values match the identified result -> `Applied`;
- result valid and Active values differ -> `Apply` remains available;
- no valid result -> idle.

Transient `Running`, `Applying` and immediate failure feedback remain runtime UI state.

## Identification settings

Identification startup/configuration values that have a clear user-facing role may be shown below the motor-parameter table instead of being mixed into the parameter/result rows.

The current GUI exposes the user-facing identification setup Parameters required by the Application preflight when the connected firmware provides them:

- the shared I/F forced-start current, `PARAM_IDENT_IF_CURRENT`;
- J/B excitation ratio, `PARAM_IDENT_JB_EXCITE_RATIO`;
- J/B excitation frequency, `PARAM_IDENT_JB_EXCITE_HZ`.

The I/F current is not a Flux-only setting. It is shared by identification workflows that require forced startup before observer-based motion. The J/B excitation controls belong here because they are intentional host-visible inputs required to start J/B identification; they are not internal fitting thresholds or observer gates.

Preferred presentation:

```text
Identification Settings

Parameter                 Value
-------------------------------------
I/F startup current       [ 0.500 ] A
J/B excitation ratio      [ 0.100 ]
J/B excitation frequency  [ 2.000 ] Hz
```

Do not add internal thresholds, observer gates, fitting constants or other algorithm details merely because they exist in firmware. A setting belongs on this page only when it is an intentional host-visible control for the identification workflow.

## Readiness and preflight

The Motor page does not locally decide whether Rs/Ls, Flux or J/B may start.

Readiness policy belongs to the Application layer as defined in `APPLICATION_PREFLIGHT.md`.

Examples include:

- required current/speed operating limits;
- valid prerequisite motor data;
- suitable motor state;
- active faults;
- conflicting tasks/resources.

The GUI may request/display structured preflight information and navigate the user to another domain such as Limits / Safety, but the authoritative check is repeated when the Action starts.

## Global motor controls

Motor identification actions are domain actions; global motor state controls remain in the application shell.

The page does not duplicate the persistent global:

```text
[ Enable / Disable ]   [ Stop ]
```

interaction. Shared shell behavior is defined in `UI_INTERACTION_RULES.md`.

An identification Action may internally require firmware/Application sequencing appropriate to that semantic operation, but the GUI must not reproduce low-level mode/action sequences itself.

## Parameter/API boundary

Motor values shown here are the same Parameters exposed in the generic Parameters page.

```text
Motor page --------+
                   +--> ParameterService --> Device Parameter API
Parameters page ---+
```

Likewise, identification buttons call semantic Application Action APIs rather than constructing raw protocol messages.

## Initial implementation boundary

The current GUI implements:

- Pole pairs, Rs, Ld, Lq, Flux, J and B Active values;
- latest valid Rs/Ls, Flux and J/B results;
- Rs/Ls, Flux and J/B Action start/completion state;
- explicit Apply after a valid result;
- shared I/F startup current when exposed by the Host schema;
- J/B excitation ratio and frequency when exposed by the Host schema.

Current limitations that must remain explicit rather than simulated:

- firmware compiled/default values are not yet exported in the current Host schema, so the Default column is `—`;
- additional identification settings appear only when they become intentional Host-visible parameters;
- detailed internal identification phases remain firmware implementation details.
