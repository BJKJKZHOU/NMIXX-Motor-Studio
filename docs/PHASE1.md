# Phase 1

> Status: Completed historical milestone. This document records the original headless core bring-up scope; it is not the current product roadmap. The repository has since added the Tauri/Svelte desktop client, Scope, domain pages and shared Application services.

## Goal

Prove the core path from a host process to the current AxDr_L firmware without any GUI.

## Scope

1. CAN FD canonical frame and USB envelope.
2. Serial transport abstraction and USB CDC implementation.
3. Transaction allocator and response matching.
4. Parameter read/write with typed values.
5. Action trigger with accepted/completed lifecycle.
6. Minimal `nmixxctl` commands for real hardware validation.

## Out of scope for Phase 1

- GUI / Tauri / Svelte;
- commissioning workflows;
- Python SDK;
- reusable automation framework;
- recorder/replay;
- Plot UI.

These exclusions describe only the original Phase 1 milestone and do not constrain the current architecture.

Telemetry decoding could be added after Parameter/Action were verified on hardware.

## Hardware validation target

```text
nmixxctl --port /dev/ttyACM0 param get motor.rs
nmixxctl --port /dev/ttyACM0 param set motor.pole-pairs 16
nmixxctl --port /dev/ttyACM0 action motor-disable
nmixxctl --port /dev/ttyACM0 action motor-enable
```
