# NMIXX Motor Studio architecture

## Product boundary

NMIXX Motor Studio is centered on a headless Rust Application Runtime.

```text
GUI -----------+
CLI -----------+--> Application API --> Application Core --> Device Core --> MCU
Automation ----+
```

The Application API is the stable product surface. No automation language is privileged.

## Layer rules

### `nmixx-core`

Owns transport-independent device capabilities:

- canonical CAN FD frame representation;
- USB CDC AXDR envelope;
- protocol transaction/response handling;
- Parameter primitives;
- Action accepted/completed semantics;
- Event decoding;
- telemetry stream decoding.

It must not depend on UI, CLI, scripting language, commissioning workflow, or a specific test procedure.

### `nmixx-app`

Owns host application semantics:

- device sessions;
- task lifecycle;
- resource arbitration;
- scope/recorder services;
- protection-aware application behavior;
- public Application API.

### clients

GUI, CLI and automation are peers. They must not implement device protocol logic.

## Current firmware adapter

The initial implementation follows the current AxDr_L wire protocol:

- 11-bit standard CAN identifier;
- message type in bits 10..6 and node in bits 5..0;
- canonical CAN FD data lengths;
- USB byte-stream envelope: `AXDR` + little-endian CAN ID + data length + complete CAN FD data field;
- Parameter and Action transported via the Parameter message family;
- finite Actions use immediate response plus asynchronous ACTION_COMPLETE event;
- FAST/NORMAL telemetry are treated by the host as streams, not as GUI-only Plot features.

`AxDr_L_Motor/tools/*.py` are not architectural dependencies.
