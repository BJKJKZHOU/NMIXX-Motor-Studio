# Control Architecture Page

## Purpose

Control Architecture is the control-system construction and structure-configuration domain.

It is a parent navigation item with three loop-specific child pages:

```text
Control Architecture
├─ Current Loop
├─ Speed Loop
└─ Position Loop
```

The parent groups the control-structure workflow. Each child page owns the complete architecture/configuration view for one loop. The product does not place all three loop diagrams into one long Control Architecture page.

It answers:

- which controller / observer / feedback strategy is selected for each loop;
- how the active control path is connected;
- which structural blocks are enabled;
- what parameters belong to those blocks.

It is not the primary repeated tuning workspace. Parameters shown here may be editable, but the page is organized around the control diagram and structure rather than around a fast tune-run-inspect loop.

Control Architecture and Control Tuning may expose the same Parameter when that Parameter is relevant in both contexts. They must use the same shared `ParameterService`; duplicate storage or page-local copies are not allowed.

## Core distinction from Control Tuning

The product boundary is:

```text
Control Architecture
    -> choose / configure the control structure
    -> controller type
    -> feedback source
    -> observer choice
    -> filters
    -> feedforward
    -> limits or structural options local to the control path
    -> parameters placed in the block where they belong

Control Tuning
    -> repeatedly tune the already selected structure
    -> expose only common / primary tuning parameters
    -> bind tuning edits to one motion experiment
    -> capture and inspect the resulting waveform
```

A Parameter may appear on both pages when both workflows need it. For example, speed-loop Kp may be editable inside the PI block on Control Architecture and also appear in the compact Speed Loop section on Control Tuning. Both views refer to the same firmware Parameter.

## Navigation and child pages

The three loop pages are peers under the Control Architecture parent:

```text
Control Architecture
│
├── Current Loop
│     controller selection
│     current-loop diagram
│     current feedback / decoupling / filters / feedforward
│     controller parameters
│
├── Speed Loop
│     controller selection
│     speed-loop diagram
│     speed feedback / observer / filters / feedforward
│     controller parameters
│
└── Position Loop
      controller selection
      position-loop diagram
      position feedback / filters / feedforward
      controller parameters
```

Selecting the parent may reopen the last selected child page. On the first visit, Current Loop is the default child unless a later product decision specifies otherwise. A separate empty overview page is not required.

This hierarchy is a navigation distinction, not a data-ownership distinction. All child pages use the same shared Application/Parameter services.

## Layout principle

Each child page is diagram-oriented.

Parameters belong near the control block or signal path they affect instead of being collected into a generic side table.

Conceptual Speed Loop child page:

```text
Speed Ref
   |
   v
[ Acc / Dec ]
   |
   v
  (+) <-------------------------------------- Speed Feedback
   |
   v
+-----------------------+
| Speed Controller      |
| Controller   [ PI v ] |
| Source       [ ...  ] |
| Bandwidth    [ ...  ] |
| Kp           [ ...  ] |
| Ki           [ ...  ] |
+-----------------------+
   |
   v
[ Output Filter ]
   |
   v
  (+) <----------------------------- [ Feedforward ]
   |
   v
Iq Ref
```

The exact diagram changes with the selected firmware capability. Do not render unsupported blocks merely to fill the page.

## Controller selection

Controller selection belongs on Control Architecture because it changes the control structure itself.

Examples of future choices may include:

```text
Current Loop
    PI
    LADRC
    ...

Speed Loop
    PI
    P + ESO
    LADRC
    ...

Position Loop
    P
    stiffness / damping
    ...
```

The GUI must derive available choices from firmware/Application capabilities or host-visible Parameters. It must not hard-code future algorithms that the connected firmware does not expose.

If the current firmware exposes only one controller type, the page may show that controller in the reserved controller-selection location. A disabled selector is acceptable as a temporary implementation placeholder, but it must not imply that unsupported choices already exist.

## Structural blocks

Control Architecture is the natural home for configuration that changes the control path rather than merely changing a frequently tuned gain.

Examples include:

- controller type;
- feedback source;
- observer type;
- filter enable / mode;
- filter frequency;
- notch-filter parameters;
- feedforward enable / type;
- feedforward gain;
- anti-windup mode;
- controller-specific structural options;
- loop-local clamps or structural limits when they are not general operating Limits / Safety parameters.

A structural block should appear where it acts in the signal path.

For example:

```text
controller output
      |
      v
[ Output Filter ]
      |
      v
next stage
```

Filter Mode / Frequency therefore belong in or beside the Output Filter block, not in an unrelated parameter panel.

## Editable parameters

Parameters on Control Architecture may be editable when the firmware exposes them as writable.

The page follows the same shared edit semantics as other Parameter-backed pages:

```text
typing -> GUI draft
Enter  -> write Device RAM and read back effective value
Esc    -> discard draft
blur   -> no commit
```

RAM values that differ from the last successfully persisted configuration retain the shared modified background until Save succeeds or the value is restored to the persisted baseline.

The fact that a Parameter is editable here does not make this the preferred repeated tuning workflow.

## Relationship to Control Tuning

Control Tuning intentionally repeats only the high-frequency tuning subset.

Typical shared Parameters:

- Current-loop Bandwidth;
- Current-loop Id/Iq Kp/Ki;
- Speed-loop Bandwidth;
- Speed-loop Kp/Ki;
- Position-loop Kp;
- Mechanical ESO Bandwidth.

Typical Architecture-only controls:

- controller type;
- feedback selection;
- filters;
- feedforward;
- algorithm-specific structural settings.

The distinction is based on workflow, not on data ownership:

```text
Control Architecture ----+
                         +--> ParameterService --> Device Parameter API
Control Tuning ----------+
Parameters page ---------+
```

## Initial implementation

The desktop GUI now exposes Control Architecture as a parent domain with three loop-specific child views:

```text
Control Architecture
├─ Current Loop
├─ Speed Loop
└─ Position Loop
```

The shell owns the child navigation and preserves the selected loop during the application session. `ControlPage.svelte` renders only the selected loop while continuing to use the same shared Parameter bindings and write path.

The current child diagrams are still the first structural version. Next implementation work should:

- keep Parameter editing inside the relevant diagram blocks;
- make actual Kp/Ki editable where firmware exposes them as writable;
- keep the controller-selection location reserved for firmware-exposed controller choices;
- clarify observer/feedback placement, especially Mechanical ESO on the Speed Loop page;
- add filters, feedforward, feedback selection and controller-specific blocks only when their firmware/Application contracts exist;
- show inter-loop relationships through clear input/output signals such as `Wm Ref` and `Iq Ref`, without forcing all three loops into one giant combined diagram.

## Non-goals

Control Architecture is not:

- a generic Parameters table;
- a replacement for Control Tuning;
- an endless live Scope;
- a page that invents unsupported algorithms, filters or observers;
- a second storage path for controller parameters.
