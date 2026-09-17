# Control Tuning Page

## Purpose

Control Tuning is a servo closed-loop tuning page. It is not the same workflow as the generic Plot page.

The page binds three things into one repeatable experiment:

1. controller / observer tuning parameters;
2. one motion command;
3. one finite waveform capture synchronized to that motion.

The intended workflow is:

> select mode -> explicitly enable from the global toolbar -> adjust parameters -> configure one motion -> Run -> inspect the complete response -> adjust again.

The GUI operates through the Application API. It must not assemble low-level protocol transactions directly.

## Global motor controls are not owned by this page

Connection / disconnection, motor Enable / Disable and other small top-level safety actions belong to the persistent application toolbar/header. They remain visible and keep the same state when the user changes pages.

Control Tuning must never automatically Enable or Disable the motor as a side effect of `Run`, page entry, page exit, parameter editing or capture control.

Enable / Disable is an explicit human safety action. In particular, software automation must not race with or override a user who is intentionally changing the enable state.

The page may read and display the global motor state, but it does not own that state transition.

`Run` is only valid when the motor is already `ENABLED`. If it is not enabled, the page reports that the user must explicitly Enable the motor from the global control area; it must not enable on the user's behalf.

## Layout

The first version uses three functional regions below the persistent application-level toolbar:

```text
Application toolbar (persistent across pages)
[ Connect / Disconnect ]   [ Enable / Disable ]   [ small top-level actions ]

+---------------------------------------------------------------+
| Control Tuning                                                |
+-----------------------------------------+---------------------+
|                                         | Current Loop        |
|                                         | Bandwidth [1000] Hz |
|                                         | Id Kp      0.0178   |
|                                         | Id Ki      532.3    |
|              Experiment                 | Iq Kp      ...      |
|              Waveform                   | Iq Ki      ...      |
|                                         |                     |
|                                         | Speed Loop          |
|                                         | Bandwidth [50] Hz   |
|                                         | Kp         ...      |
|                                         | Ki         ...      |
|                                         |                     |
|                                         | Position Loop       |
|                                         | Kp         [5.0]    |
|                                         |                     |
|                                         | Mechanical Observer |
|                                         | Bandwidth [100] Hz  |
+-----------------------------------------+                     |
| Motion Command                          |                     |
| Mode [ Position v ]                     |                     |
| Target [...]  Speed [...]               |                     |
| Acc [...]     Dec [...]                 |                     |
|                         [ Run ]          |                     |
+-----------------------------------------+---------------------+
```

The left side is the experiment waveform area. The lower-left area defines the motion for the next experiment. The right side contains tuning parameters and the active gains derived by firmware.

## Tuning parameter semantics

The primary writable values are physical / design parameters instead of directly exposing every coupled gain.

### Current Loop

Writable:

- current-loop bandwidth, Hz.

Read-only active values:

- Id Kp;
- Id Ki;
- Iq Kp;
- Iq Ki.

Firmware continues to derive the current-loop gains from active `Rs`, `Ld`, `Lq` and the requested bandwidth.

### Speed Loop

Writable:

- speed-loop bandwidth, Hz.

Read-only active values:

- Kp;
- Ki.

Firmware continues to derive the speed-loop gains from active `J`, `B`, `Flux`, pole pairs and the requested bandwidth.

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
  -> user explicitly enables from global toolbar
ENABLED
  -> parameters may still be edited
  -> user presses Run for an experiment
RUN
  -> tuning controls locked / firmware rejects tuning writes
  -> experiment finishes and returns to ENABLED
ENABLED
  -> user adjusts parameters and runs the next experiment
```

Parameter editing must not cause implicit Enable, Disable, Run or Stop actions.

## Run behavior

The page keeps edited values as pending/dirty values. When the user presses `Run`, the Application API performs one complete experiment using a consistent parameter set, but assumes the user has already explicitly put the motor in `ENABLED` state:

```text
verify motor state == ENABLED
        |
        +-- no --> show "Enable motor first" and stop
        |
       yes
        v
write dirty tuning parameters
        |
        v
read back active parameters / gains
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
motion complete / settled
        |
        v
return to ENABLED / stop motion
        |
        v
post-motion capture
        |
        v
stop capture and retain waveform
```

There is deliberately no implicit `Enable` or `Disable` in this sequence.

While an experiment is running, the tuning controls should be locked in the first implementation. Online gain editing can be added later only if there is a concrete need.

## Experiment capture

This page does not use an endless scrolling Plot session.

A capture is finite and synchronized to one motor experiment. It includes time before and after the commanded movement, for example:

- pre-motion capture: 0.5 s;
- motion: actual duration;
- post-motion capture: 1.0 s.

The exact durations can become configurable later. The post-motion interval is important because position-loop tuning must show settling and any vibration after the target is reached.

The waveform remains on screen after capture ends so the user can inspect the complete response before changing the next parameter set.

## Default waveform groups

The user may add/remove channels later, but each motion mode should start with a useful default set.

### Position experiment

Recommended defaults:

- Position Ref;
- Encoder Position;
- Wm Ref;
- Mechanical ESO Wm;
- Iq Ref;
- actual Iq;
- Mechanical ESO disturbance torque when available.

### Speed experiment

Recommended defaults:

- Wm Ref;
- Mechanical ESO Wm;
- encoder difference speed when available;
- Iq Ref;
- actual Iq;
- Mechanical ESO disturbance torque when available.

### Current-loop experiment

Recommended defaults:

- Iq Ref / Iq;
- Id Ref / Id;
- Uq / Ud.

## Motion Command

The lower-left form depends on the selected mode.

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

## Difference from Plot

`Plot` and `Control Tuning` may reuse the same lower-level streaming/capture infrastructure, but they have different product behavior.

Plot:

- continuous observation;
- freely selected variables;
- open-ended duration;
- generic debugging tool.

Control Tuning:

- one finite capture per experiment;
- tuning parameters, motion command and waveform are bound together;
- capture starts before the motion and ends after settling;
- intended specifically for closed-loop tuning.

## Firmware contract required by this page

The firmware should expose these writable design parameters through the Parameter Core:

- Current-loop bandwidth;
- Speed-loop bandwidth;
- Position-loop Kp;
- Mechanical ESO bandwidth.

These tuning parameters must be writable in `DISABLED` and `ENABLED`, and rejected in `RUN`.

It should also expose these read-only active controller values:

- Id Kp / Ki;
- Iq Kp / Ki;
- Speed Kp / Ki.

Observer diagnostic states and old encoder-difference speed can be added as read-only Plot channels when the tuning workflow needs them.

Motor parameters (`Rs/Ld/Lq/Flux/J/B/Pp`), limits and motion configuration remain separate parameters and are reused by this page rather than duplicated.
