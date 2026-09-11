# ADR 0002: Canonical frame model separates protocol from transport

- Status: Accepted
- Date: 2026-09-11

## Context

The current AxDr_L firmware defines a canonical CAN FD wire format. USB CDC does not define a second application protocol; it only wraps one complete CAN FD frame in an `AXDR` byte-stream envelope. Native CAN FD support is also planned.

If protocol code depends directly on serial-port framing, adding CAN FD would duplicate logic and couple the application to one transport.

## Decision

`nmixx-core` uses one canonical frame model between protocol and transport adapters.

```text
Application / Device semantics
            |
         Protocol
            |
      CanFdFrame model
        /         \
 USB CDC         native CAN FD
 envelope          adapter
```

USB CDC is responsible only for byte-stream framing/resynchronization. Protocol code works only with canonical frames and does not know which transport carried them.

## Consequences

- Parameter, Action, Event and Stream protocol logic is implemented once.
- USB and native CAN FD can coexist without separate application stacks.
- Transport failures and protocol failures remain separately diagnosable.
- Transport tests can use fake/in-memory frame transports without physical hardware.
