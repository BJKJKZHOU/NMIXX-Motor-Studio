# NMIXX Motor Studio

NMIXX Motor Studio is a Rust-first motor-control host runtime and tooling stack.

The long-term product boundary is:

```text
GUI / CLI / Automation
        |
        v
Application API
        |
        v
Rust Application Core
        |
        v
Rust Device Core
        |
        v
Protocol / Transport
        |
        v
Motor Controller
```

The GUI is a client, not the product core. Automation is intentionally language-agnostic.

## Workspace

- `nmixx-core`: wire format, transport-independent protocol, device primitives.
- `nmixx-app`: application semantics and future public Application API.
- `nmixx-cli`: CLI client / early API validation surface.

## Phase 1

The first phase targets the current AxDr_L firmware protocol without importing its temporary host scripts:

1. canonical CAN FD frame model;
2. USB CDC `AXDR` envelope;
3. Parameter request/response primitives;
4. Action accepted/completed semantics;
5. then serial transport and a minimal CLI.

The current firmware remains the protocol authority. `tools/*.py` in the firmware repository are treated as disposable test scripts, not as the host architecture source.
