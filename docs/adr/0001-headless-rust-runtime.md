# ADR 0001: Headless Rust runtime with peer clients

- Status: Accepted
- Date: 2026-09-11

## Context

NMIXX Motor Studio needs a modern GUI, a first-class CLI, and open-ended automation. Automation must not be tied to Python, and no client should directly own the motor-controller transport or duplicate protocol behavior.

## Decision

The product core is a headless Rust Application Runtime.

GUI, CLI and automation are peer clients above one Application API. Device communication, protocol handling, task lifecycle and shared application rules stay below that API.

```text
GUI -----------+
CLI -----------+--> Application API --> Rust Application Core --> Device Core
Automation ----+
```

The current desktop GUI uses Tauri + Svelte, but Tauri is a client implementation choice rather than an architectural dependency of the runtime.

## Consequences

- The system remains useful without a GUI.
- Automation can use Python, shell, Rust, JavaScript, CI, or future clients.
- Changing the GUI technology does not require rewriting device communication.
- Application API design is treated as a product concern and must be kept coherent.
- A runtime/session must arbitrate access when multiple clients are active.
