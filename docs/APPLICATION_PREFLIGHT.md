# Application Action Preflight

## Purpose

NMIXX Motor Studio exposes motor parameters through one shared `ParameterService`, while workflow pages organize those parameters around real engineering tasks.

Actions that may energize or move the motor must not rely on each GUI page to decide whether the system is ready. Readiness checks belong in the Application layer so GUI, CLI and automation receive the same behavior.

This document defines the boundary between:

- domain pages such as Motor and Limits / Safety;
- `ParameterService`;
- Application Action API;
- action preflight checks.

It is a host-application design rule. Identification algorithms and firmware business logic do not depend on GUI pages.

## Core rule

Business pages organize Parameter values for people. The Parameters page exposes the complete Parameter registry. Both use the same `ParameterService` underneath.

```text
Parameters page ---------+
Motor page --------------+
Encoder page ------------+----> ParameterService ----> Device Parameter API
Limits / Safety page ----+
Control page ------------+
```

A value has one source of truth and one read/write path.

For example, `PARAM_LIMIT_I_MAX` shown on the Limits / Safety page is the same Parameter shown in the generic Parameters table. The Limits page does not own a second copy, cache, persistence format, or write path for that value.

## Action start path

A user-facing action is started through the Application Action API rather than directly from a GUI component.

```text
                 Motor Page
                     |
                     | Flux Action
                     v
           Application Action API
                     |
               Preflight Check
                     |
          +----------+----------+
          |                     |
          v                     v
  ParameterService        Runtime state
          |                     |
  I_MAX / WM_MAX / ...    motor state / fault / ...
          |                     |
          +----------+----------+
                     |
              ready / blocked
                |         |
                v         v
          Action Start   structured issue
                              |
                              v
                  GUI can navigate to
                    Limits / Safety
```

The GUI may call a read-only preflight method in advance to improve presentation, but the Application layer must execute the authoritative preflight again when the action is actually started.

This prevents CLI or automation from bypassing the same readiness rules.

## Responsibility boundaries

### `ParameterService`

`ParameterService` owns access to host-visible Parameters.

It should:

- read Parameter values;
- write Parameter values;
- expose Parameter metadata;
- provide subscriptions/cache behavior where required by the Application Runtime.

It should not know:

- which GUI page displays a Parameter;
- whether Flux identification is allowed to start;
- which workflow should be suggested to a user;
- commissioning policy.

### Application Action API

The Application Action API owns the semantic operation requested by clients.

Examples:

```text
motor.enable
motor.run
action.start(RsLs)
action.start(Flux)
action.start(JB)
```

Before an operation that may energize or move the motor starts, the Application layer checks the prerequisites required by that operation.

The Action API is shared by GUI, CLI and automation.

### Preflight

Preflight owns Application-level readiness policy.

A preflight may inspect:

- required Parameter values;
- motor state;
- active faults;
- previously identified motor data;
- resources or tasks already in use.

Preflight does not implement the identification algorithm itself.

### GUI business pages

A business page organizes existing Application API and Parameter concepts around a human workflow.

For example, Limits / Safety may group:

```text
Current
  Current limit

Speed
  Maximum speed
  Acceleration
  Deceleration

Position
  Motion / position limits
```

The page may explain why a value matters and may highlight missing prerequisites, but it does not become the authority that decides whether an Action is safe to start.

## Limits are derived from Parameter values

The application must not infer that limits are configured because the user visited the Limits page or clicked a page-specific Save button.

The same Parameters may be changed through:

- Limits / Safety;
- the generic Parameters page;
- CLI;
- automation;
- another future client.

Therefore readiness is derived from the current Parameter values.

Conceptually:

```text
CurrentLimitReady = PARAM_LIMIT_I_MAX is valid
SpeedLimitReady   = PARAM_LIMIT_WM_MAX is valid
```

Validation may include conditions such as finite values, positive ranges and firmware-defined absolute bounds.

A separate `limits_page_visited` or equivalent flag must not be used as the source of truth.

## Action-specific prerequisites

Do not require every Limits / Safety field for every motor-related operation. Each Action checks only the prerequisites that are relevant to that operation.

Initial intended mapping:

| Action | Required operating limits | Other important prerequisites |
| --- | --- | --- |
| Rs/Ls identification | Current limit | valid motor setup and suitable motor state |
| Flux identification | Current limit, speed limit | valid Rs/Ls result and suitable motor state |
| J/B identification | Current limit, speed limit | valid Flux result and suitable motor state |
| Torque run | Current limit, speed limit | suitable motor/control state |
| Speed run | Current limit, speed limit | suitable motor/control state |
| Position run | Current limit, speed limit, relevant motion limits | suitable motor/control state |

The exact Parameter IDs and validity rules should follow the firmware schema and may evolve. The architectural rule is that prerequisite ownership remains in the Application layer.

## Structured preflight result

A preflight failure must not collapse to a single boolean or an opaque text error.

The Application API should preserve enough structure for different clients to present or automate the recovery path.

Conceptual model:

```text
PreflightResult
  ready: bool
  issues[]
    kind
    parameter_id?       // when the problem refers to one Parameter
    reason
    suggested_domain?   // e.g. LimitsSafety, Motor, Encoder
```

Example:

```text
ready = false

issues:
  - kind: MissingConfiguration
    parameter_id: PARAM_LIMIT_I_MAX
    reason: Current limit is not configured
    suggested_domain: LimitsSafety

  - kind: MissingConfiguration
    parameter_id: PARAM_LIMIT_WM_MAX
    reason: Maximum speed is not configured
    suggested_domain: LimitsSafety
```

The GUI can then render a focused message such as:

```text
Flux identification cannot start yet.

Configure:
- Current limit
- Maximum speed

[Configure Limits]
```

Selecting the recovery action may navigate to Limits / Safety and highlight the relevant fields.

CLI and automation can consume the same structured issues without depending on GUI-specific text.

## GUI behavior

The GUI is responsible for presentation, not for owning the safety decision.

Allowed:

```text
Motor page
  -> preflight Flux for display
  -> show "Requires motor limits"
  -> user clicks Flux
  -> Application starts Flux
  -> Application performs authoritative preflight again
```

Not allowed:

```text
Motor page
  -> directly reads I_MAX and WM_MAX
  -> locally decides Flux is allowed
  -> bypasses Application policy
```

When a missing prerequisite belongs naturally to another business page, the GUI should offer navigation rather than duplicating the configuration controls everywhere.

For motor limits this means:

```text
Motor page
    |
    | missing I_MAX / WM_MAX
    v
"Configure Limits"
    |
    v
Limits / Safety page
    |
    +--> same ParameterService
```

## Identification-specific application

For the current Motor page, identification Actions follow the same architecture as other motor-moving operations.

```text
Motor parameter row
  Pole pairs
  Rs
  Ld
  Lq
  Flux
  J
  B

Identification Actions
  Rs/Ls
  Flux
  J/B
```

The identification implementation remains responsible for its own internal phases, observers, fitting logic and algorithm failures.

The host Application preflight is only responsible for deciding whether the requested Action may begin with the current configuration and runtime state.

The firmware identification code must not know that the host has a page named `Limits / Safety`.

## Valid versus explicitly confirmed limits

There are two possible product semantics:

1. **Valid limits**: current Parameter values satisfy their validity rules.
2. **Explicitly confirmed limits**: the user has deliberately reviewed and confirmed the current operating limits for the motor.

The initial implementation should use the first model unless a commissioning workflow clearly requires explicit confirmation.

Do not introduce a confirmation flag merely because firmware Parameters have compiled defaults.

If explicit confirmation is introduced later, it belongs to Application commissioning state rather than firmware Parameter state, and it must be tied to the values that were confirmed. Changing a relevant limit through any client must invalidate that confirmation.

Conceptually:

```text
LimitsConfirmed =
    LimitsValid
    && current_limit_values == last_confirmed_limit_values
```

This is intentionally deferred from the first implementation.

## Implementation guidance

Prefer explicit, readable checks before introducing a generic prerequisite-rule framework.

For example, an initial implementation may contain dedicated functions such as:

```text
preflight_rs_ls()
preflight_flux()
preflight_jb()
preflight_motor_run(...)
```

These functions may reuse small validation helpers such as current-limit or speed-limit validation.

Do not build a dynamic rule engine solely to remove a small amount of duplicated code. The important architectural boundary is centralized Application ownership, not maximum abstraction.

## Non-goals

This design does not:

- create another Parameter database;
- move firmware safety checks to the host;
- replace hardware current/voltage protections;
- expose identification internal state machines through Parameter;
- force all motor Actions to require the same limits;
- require a wizard workflow;
- prevent expert users from editing the same values in the generic Parameters page.

Firmware and hardware protections remain authoritative at their own layers. Application preflight adds workflow-level readiness and consistent client behavior; it is not a substitute for device-side safety.

## Summary

The stable relationship is:

```text
ParameterService
    |
    +-- one source of truth for host-visible Parameter access

Application Preflight
    |
    +-- understands what each Action requires before it may start

GUI business pages
    |
    +-- organize Parameters and Actions around human workflows
    +-- present structured preflight failures
    +-- navigate the user to the right configuration page
```

This keeps the Parameters page complete, keeps domain pages useful, and keeps motor-motion readiness policy consistent across GUI, CLI and automation.
