# ADR 0004: Single-owner device session runtime

- Status: Accepted
- Date: 2026-09-11

## Context

NMIXX Motor Studio must support GUI, CLI and automation at the same time without allowing multiple clients to compete for the same serial/CAN transport. Request code must also continue receiving asynchronous Action completion, protection events and telemetry while it waits for a matching response.

A design where every API call reads the transport itself would create races and make future multi-client behavior fragile.

## Decision

`nmixx-app::DeviceSession` owns one `FrameTransport` on one dedicated worker thread.

Clients hold cloneable `DeviceSession` handles. A clone only clones the command sender; it never clones or reopens the transport.

The worker owns:

- the transport;
- the transaction table;
- the Action tracker;
- inbound frame decoding;
- event/stream subscriber fan-out.

Initial requests are serialized deliberately. The public Application-facing API does not expose this scheduling choice, so the worker may later support multiple pending requests without changing clients.

```text
GUI / CLI / Automation
        |
  DeviceSession handles
        |
   command channel
        v
 single worker thread
   |      |      |
 Txn   Action   RX dispatch
        |
   FrameTransport
        |
       MCU
```

## Consequences

- Exactly one component reads each device transport.
- Asynchronous events can be dispatched while a request waits for its response.
- GUI, CLI and automation share one device state rather than opening competing serial ports.
- The core protocol remains independent of the threading/runtime model.
- The first implementation can stay synchronous and dependency-light.
- Higher concurrency can be introduced inside the worker later without changing the Application API.
