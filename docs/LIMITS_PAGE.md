# Limits / Safety Page

## Purpose

The Limits / Safety page is the human-facing configuration view for motor operating boundaries. It does not own a second parameter store; all values continue to use the shared `ParameterService` and firmware Parameter API.

The page is intentionally narrow. It shows real limit sources and optional position soft limits. Motion-profile settings such as commanded trajectory speed, acceleration, and deceleration do not belong here.

## Operating limits

Current and speed limits have two independent sources:

```text
Operating Limits

Parameter          User Limit              Hardware Limit
----------------------------------------------------------
Current            [ 8.000 ] A             10.000 A
Maximum speed      [ 400.0 ] rad/s         300.0 rad/s
```

- **User Limit** is editable and expresses the user's preferred operating boundary.
- **Hardware Limit** is read-only and expresses the device-side absolute boundary.
- the active operating value is the stricter of the two.

Conceptually:

```text
CurrentLimit = min(UserCurrentLimit, HardwareCurrentLimit)
SpeedLimit   = min(UserSpeedLimit, HardwareSpeedLimit)
```

A user value higher than the hardware value is not inherently an error. The firmware should preserve the configured user value and enforce the hardware limit at runtime.

The GUI uses two different low-intensity visual states:

- a neutral background lift for a configured user value;
- a distinct accent background for the source that is currently active.

These are source/selection states, not success/error colors.

## Bus voltage safety window

DC-bus voltage protection is shown as a separate safety window rather than as another User/Hardware operating-limit source.

```text
Bus Voltage

Minimum                  Actual                   Maximum
[ 8.0 ] V                14.5 V                   [ 30.0 ] V
Undervoltage limit       DC bus voltage           Overvoltage limit
```

The three values have distinct ownership:

- **Minimum** is the persistent firmware undervoltage software-protection threshold.
- **Actual** is the read-only sampled DC-bus voltage (`Vbus`).
- **Maximum** is the persistent firmware overvoltage software-protection threshold.

The GUI may visually indicate when Actual is outside the configured window, but it does not own the protection decision. Firmware `Protection` remains authoritative for rejecting Enable and for latching undervoltage/overvoltage Stop faults while enabled or running.

Threshold writes are allowed only while the motor is `DISABLED`, and firmware enforces:

```text
0 < VbusMin < VbusMax
```

A missing or low bus voltage while already `DISABLED` does not by itself latch an undervoltage fault. Instead, Enable is rejected while Actual is outside the valid window. Once the motor is enabled/running, a sustained out-of-window voltage becomes the corresponding protection Stop condition.

## Motion profile is not a limit source

`Motion_Config.Wm_Max`, acceleration, and deceleration describe how a requested motion should be generated. They are command/profile settings rather than independent safety boundaries.

The intended layering is:

```text
User command / trajectory
        |
        v
Motion planner / profile
        |
        v
Operating-limit clamp
        |
        +-- User Limit
        +-- Hardware Limit
        v
Final reference
```

Therefore the Limits / Safety page must not treat motion speed as a third speed-limit source.

## Optional position limits

Position limits are optional software travel boundaries for applications such as linear stages, screw drives, rotary joints, and other mechanisms with a finite mechanical range.

Normal continuously rotating motors do not require them, so position limits are disabled by default and are not a prerequisite for ordinary motor operation.

The intended page section is:

```text
Position Limits

Zero reference        Set / Not set
Enable                Off / On
Minimum position      Turn + Theta
Maximum position      Turn + Theta
```

Minimum and maximum positions use the same user mechanical coordinate and `position` representation already used by the firmware (`Turn + Theta`).

## Position limits require a valid zero reference

Position limits are defined relative to mechanical zero. They must not become active before that coordinate reference is established.

The fundamental rule is:

```text
ZeroValid == false
    -> Position Limits cannot be enabled

ZeroValid == true
    -> Position Limits may be enabled
```

When enabled, the configured range must also be valid:

```text
PositionLimitActive =
    PositionLimitEnable
    && ZeroValid
    && PositionMin < PositionMax
```

The GUI may disable the Enable control when zero is not valid, but this rule must also be enforced by firmware/Application semantics so CLI, automation, or the generic Parameters page cannot bypass it.

## Zero ownership

The Limits / Safety page does not establish mechanical zero.

Zero-setting and homing belong to the Encoder / calibration workflow. Limits / Safety only consumes the resulting read-only zero-valid state.

Conceptually:

```text
Encoder / Homing
    -> establish mechanical zero
    -> ZeroValid = true

Limits / Safety
    -> may enable Position Limits
```

## Restart semantics

Zero validity depends on the feedback/mechanical system.

An absolute encoder with a persistent mechanical reference may retain a valid zero after restart. A relative encoder or a machine that requires homing may lose zero validity after restart.

Therefore a persisted position-limit configuration must never imply that the limits are active by itself.

Even if the user previously configured:

```text
PositionLimitEnable = true
```

the actual runtime state remains dependent on `ZeroValid`.

## Planned firmware-facing parameters

Exact IDs are deferred until the firmware work is implemented, but the intended concepts are:

```text
Position zero valid     read-only
Position limit enable   read/write
Position minimum        position, read/write
Position maximum        position, read/write

Hardware current limit  read-only
Hardware speed limit    read-only
```

The firmware should continue to own final enforcement.

## Current GUI state

The GUI already reserves the Position Limits section so the final information architecture is visible before firmware support lands.

Until the required firmware Parameters exist:

- the whole Position Limits section is visually muted;
- Zero reference is unavailable;
- Enable is fixed Off and disabled;
- Minimum/Maximum inputs are disabled;
- no synthetic or guessed values are written to the device.

This is an intentional capability placeholder rather than a simulated implementation.

## Current firmware gaps

Bus-voltage minimum/actual/maximum support is now defined by the page contract above. Remaining deferred firmware work:

- expose hardware current and speed limits as read-only Parameters;
- allow a user limit to exceed the hardware limit without rejecting the write solely for that reason;
- remove `Motion_Config.Wm_Max` from the operating-limit source calculation;
- expose mechanical zero validity;
- add optional position-limit enable/min/max semantics and enforce their zero dependency.
