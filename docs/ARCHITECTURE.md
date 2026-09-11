# NMIXX Motor Studio architecture

## Product boundary

NMIXX Motor Studio is centered on a headless Rust Application Runtime.

```text
GUI -----------+
CLI -----------+--> Application API --> Application Core --> Device Core --> MCU
Automation ----+
```

The Application API is the stable product surface. No automation language is privileged.

## Maintainability goals

The project is expected to grow from a CLI-first host tool into a long-lived motor-control engineering application. Maintainability is therefore a first-class requirement, not a later cleanup task.

The architecture follows these rules:

1. **Dependencies point inward.** Clients depend on Application API, Application depends on Device Core, Device Core depends on protocol/wire/transport abstractions. Lower layers never depend on GUI, CLI, scripting, or commissioning workflows.
2. **One owner for device I/O.** A device connection is owned by one runtime/session. GUI, CLI and automation never open competing transports for the same session.
3. **Wire details stay in the core.** CAN IDs, DLC rules, USB framing, transaction matching and payload layouts must not leak into Application API or clients.
4. **Transport and protocol remain independent.** USB CDC, native CAN FD and future transports carry the same protocol frame model through adapters.
5. **Application behavior is shared.** State checks, resource arbitration, protection-aware behavior, task lifecycle and error semantics belong in `nmixx-app`, not separately in each client.
6. **Long operations use one Task model.** Identification, recording, replay and future commissioning workflows expose common lifecycle/progress/cancellation semantics.
7. **Automation is composition, not core policy.** Repeat-N tests, quality thresholds and commissioning sequences are normally built by calling the Application API instead of being hard-coded into the device core.
8. **Stable data contracts are generated where practical.** Firmware Parameter/Action schema should generate host IDs and metadata rather than requiring duplicated hand-maintained tables.
9. **No hidden global state.** Session, transport, task ownership and subscriptions are explicit objects with clear lifetimes.
10. **Errors preserve layer context.** Transport, wire/protocol, device and application failures remain distinguishable so failures can be diagnosed without string parsing.

## Layer rules

### `nmixx-core`

Owns transport-independent device capabilities:

- canonical CAN FD frame representation;
- USB CDC AXDR envelope;
- transport interfaces and adapters;
- protocol transaction/response handling;
- Parameter primitives;
- Action accepted/completed semantics;
- Event decoding;
- telemetry stream decoding.

It must not depend on UI, CLI, scripting language, commissioning workflow, or a specific test procedure.

Suggested internal dependency direction:

```text
wire <- transport
  ^        |
  +--- protocol <- device
```

Protocol code may use the canonical frame model but must not know whether a frame arrived through USB CDC or native CAN FD.

### `nmixx-app`

Owns host application semantics:

- device sessions and connection ownership;
- task lifecycle;
- resource arbitration;
- scope/recorder services;
- protection-aware application behavior;
- stable public Application API.

The Application API should expose domain concepts such as `parameter.get`, `motor.enable`, `action.start`, `stream.subscribe` and `task.cancel`, not raw CAN IDs or payload bytes.

### clients

GUI, CLI and automation are peers. They must not implement device protocol logic or duplicate application policy.

A client may format values, render a progress bar, or compose a workflow, but it must not reimplement transaction matching, action completion handling, protection rules or transport framing.

## Concurrency and ownership

The runtime owns the transport and demultiplexes received frames into responses, events and streams.

```text
Transport reader
      |
      v
Frame decoder
      |
      +--> transaction router --> request futures
      +--> event bus -----------> application subscribers
      +--> stream router --------> scope / recorder / clients
```

Only one component reads from a transport. Request code never performs its own competing read loop.

## Public API stability

Internal Rust module layout may evolve, but the external Application API is treated as a product contract. Breaking API changes should be intentional and versioned.

Raw/debug access may exist for engineering use, but normal clients should use typed semantic endpoints.

## Testing strategy

- `wire`: deterministic unit tests for framing, DLC, padding and stream resynchronization.
- `protocol`: request/response and event routing tests using an in-memory fake transport.
- `device`: typed Parameter/Action tests against scripted protocol fixtures.
- `application`: session/task/resource-arbitration tests without physical hardware.
- hardware tests: explicit integration tests, never the only validation path.

This separation is important so most regressions can be caught without connecting a motor controller.

## Architecture decisions

Long-lived or non-obvious architecture choices are recorded under `docs/adr/` as Architecture Decision Records. An ADR records context, decision and consequences so future refactors do not accidentally remove an important boundary.

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
