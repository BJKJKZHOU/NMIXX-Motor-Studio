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
Operating Limits
  Current limit
  Maximum speed

Motion Limits
  Motion speed
  Acceleration
  Deceleration
```

The Limits / Safety page is intentionally a configuration page. It does not show which Action uses a limit and does not add workflow-specific warnings or explanations to normal page content.

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

A separate `limits_page_visited` or equivalent flag must not be used as the source of truth.

## Limits / Safety page value model

The Limits / Safety page presents the actual limit sources rather than a synthetic `Configured / Effective` pair.

For operating limits, the intended presentation is:

```text
Operating Limits

Parameter          User Limit              Hardware Limit
----------------------------------------------------------
Current            [ 8.000 ] A             10.000 A
Maximum speed      [ 400.0 ] rad/s         300.0 rad/s
```

The columns have explicit meanings:

- **User Limit** is the user-configured limit and is editable.
- **Hardware Limit** is the device-side motor/hardware limit and is read-only.
- the GUI does not replace either source with a single derived value.

For speed, the motion planner adds another possible limiting source:

```text
Motion Limits

Parameter          Value
-------------------------------
Motion speed       [ 250.0 ] rad/s
Acceleration       [ 100.0 ] rad/s^2
Deceleration       [ 100.0 ] rad/s^2
```

The active runtime limit is conceptually the minimum applicable source:

```text
CurrentLimit = min(UserCurrentLimit, HardwareCurrentLimit)

SpeedLimit = min(
    UserSpeedLimit,
    HardwareSpeedLimit,
    MotionSpeedLimit
)
```

The UI does not need to print this formula or add explanatory text to the page. It expresses the result visually by highlighting the source that is currently limiting the system.

### User values may exceed hardware limits

The intended product semantics are that a user-configured limit may be higher than the hardware/device limit.

Example:

```text
User current limit      20 A
Hardware current limit  10 A
Actual limiting source  Hardware
```

This is not treated as a configuration error in the Limits page:

- do not show a warning merely because the user value exceeds the hardware value;
- do not color the value red;
- do not reject it at the host UI layer;
- retain and display the user's configured value;
- visually mark the hardware value as the currently active limiting source.

The device-side control path remains responsible for enforcing the stricter limit.

### Visual state semantics

The page uses two distinct low-intensity visual states. They must not be represented by the same background treatment.

**Configured state**

Indicates that a user-editable value has been configured. Use a subtle neutral background lift. It does not mean that the value is currently limiting the system and it does not mean that the value has been validated as safe by the GUI.

**Active limiting source**

Indicates which source currently determines the runtime limit. Use a visually distinct, low-saturation accent background.

Examples:

```text
User = 8 A, Hardware = 10 A

User Limit              Hardware Limit
[ 8 A ]                 10 A
  configured + active
```

```text
User = 20 A, Hardware = 10 A

User Limit              Hardware Limit
[ 20 A ]                [ 10 A ]
  configured               active
```

For speed, if the motion limit is lower than both user and hardware limits, neither operating-limit cell is marked active; the Motion speed row is the active limiting source instead.

Do not use success/error semantics such as green = valid or red = invalid for this source selection. The highlight communicates which value is in force, not a pass/fail judgement.

### Current firmware mismatch

The current AxDr_L firmware schema still constrains `PARAM_LIMIT_I_MAX` and `PARAM_LIMIT_WM_MAX` with `Motor_Lim` as a maximum Parameter range. That means a host write above the device limit is currently rejected before the runtime `min(...)` limiting logic can preserve the larger user value.

This does not match the intended Limits page semantics above.

The firmware change is intentionally deferred. When the firmware is updated, the intended direction is:

- user limits remain positive/finite configuration values;
- they are not rejected solely for exceeding the corresponding device limit;
- runtime control continues to enforce the stricter source;
- hardware/device limits are exposed as read-only host-visible values so the Limits page can display them directly;
- existing effective-limit values may remain available for diagnostics/API use even though the page does not need a separate Effective column.

Until that firmware change is made, the GUI implementation must not pretend that higher-than-hardware user values are already supported by the device.

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

The GUI may provide navigation to the relevant business page. The destination page itself remains a normal configuration page; it does not need to render Action-specific context such as `required by Flux`.

CLI and automation can consume the same structured issues without depending on GUI-specific text.

## GUI behavior

The GUI is responsible for presentation, not for owning the safety decision.

Allowed:

```text
Motor page
  -> user requests Flux
  -> Application performs preflight
  -> if a required limit is missing, GUI offers navigation to Limits / Safety
```

Not allowed:

```text
Motor page
  -> directly reads I_MAX and WM_MAX
  -> locally decides Flux is allowed
  -> bypasses Application policy
```

When a missing prerequisite belongs naturally to another business page, the GUI may offer navigation rather than duplicating the configuration controls everywhere.

After navigation, Limits / Safety remains a plain configuration page. It does not need:

- `required by Flux` labels;
- missing-parameter banners;
- Action-specific field highlighting;
- per-limit explanations of which operation consumes a value.

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

The Limits / Safety page additionally presents the independent User, Hardware, and Motion limit sources and visually indicates which source is currently active, without turning normal source selection into a warning/error state.

This keeps the Parameters page complete, keeps domain pages useful, and keeps motor-motion readiness policy consistent across GUI, CLI and automation.
