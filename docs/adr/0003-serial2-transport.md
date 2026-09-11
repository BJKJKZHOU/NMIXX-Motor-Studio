# ADR 0003: Use serial2 behind the transport boundary

- Status: Accepted
- Date: 2026-09-11

## Context

The first hardware transport is USB CDC presented as a serial port on Windows and POSIX systems. The serial implementation must be cross-platform and stable, but it must not become part of the public NMIXX Motor Studio architecture.

## Decision

Use the `serial2` crate for the first USB CDC serial adapter.

The dependency is isolated inside `nmixx-core::transport::UsbCdcTransport`, behind the project-owned `FrameTransport` trait. Protocol and application code never depend on `serial2` types.

The adapter remains synchronous. Concurrency and the single-RX-owner execution model belong to the runtime/application layer rather than the transport trait.

## Consequences

- Windows and POSIX serial access use one small cross-platform dependency.
- Replacing the serial library does not change Protocol or Application APIs.
- USB CDC remains an adapter that emits canonical `CanFdFrame` values.
- The runtime can later move blocking transport I/O to a dedicated thread without converting protocol code to an async framework.
- Native CAN FD can implement the same `FrameTransport` boundary independently.
