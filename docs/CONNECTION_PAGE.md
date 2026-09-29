# Connection Page

> **Normative page contract.** This document defines the intended Connection workflow. Do not change it merely to match a temporary implementation. See `docs/README.md`.

## Purpose

Connection establishes and tears down the single device session used by the desktop.

The page owns transport/session configuration. It does not own motor Run, motor Stop,
Parameter persistence, Scope recording, Identification, Motion or Tuning.

A successful Connect means the Application session is ready for normal product
workflows, not merely that a serial port was opened.

## State ownership

The desktop owns one connection at a time.

```text
Connection page
    |
    v
ApplicationSession
    |
    v
DeviceSession
    |
    v
transport
```

GUI, Automation and normal CLI workflows must not create another transport for the
same desktop session.

Connection-page form values such as the selected endpoint and HostSchema path are
Host presentation/configuration state. They may survive page navigation without
becoming device state.

## Transport configuration

The current desktop exposes Serial / USB CDC.

The Connection page contains:

- transport type;
- endpoint/port;
- HostSchema source while it remains an explicit development setting;
- Refresh ports;
- Connect / Disconnect;
- connected device/session capability summary.

Future CAN FD or other transports may add transport-specific fields in this page.
They must still create the same ApplicationSession abstraction rather than a
parallel application path.

## Endpoint refresh

Refreshing available endpoints is discovery only.

It must not:

- connect automatically;
- disconnect an existing session;
- issue motor actions;
- perform Parameter writes;
- start Scope or Tuning.

If the current endpoint remains available, refreshing should preserve it. Otherwise
the page may select a reasonable discovered default while leaving the final choice
visible to the user.

## HostSchema path

The HostSchema path is an explicit development/configuration input.

It is Host state, not a firmware Parameter.

The last successfully entered/used path may be persisted locally so navigating away
or restarting the desktop does not force repeated manual entry.

Restoring the path must not connect automatically.

A missing, unreadable or incompatible HostSchema causes Connect to fail explicitly.

## Connect semantics

One user Connect request creates at most one new ApplicationSession.

A successful Connect performs the initialization required before business pages can
treat the session as usable:

```text
open transport/session
    ->
load/validate HostSchema
    ->
initial authoritative Parameter read
    ->
discover device/application capabilities
    ->
start connection-owned runtime telemetry baseline
    ->
publish connected session to the GUI
```

The exact internal order may evolve, but the following observable rules are
normative:

- business pages must not see a half-initialized connected session;
- the initial Parameter population is authoritative device RAM state;
- frontend Parameter initialization mirrors the Application-owned cache rather
  than performing a second independent full read;
- Plot/runtime capability values come from the connected device/Application
  contract rather than GUI constants;
- connection may start the shared baseline telemetry required by the status bar;
- connection must not create a user Scope recording;
- connection must not implicitly Enable, Run, start Motion, Identification,
  Phase Search, Tuning or Automation;
- connection must not implicitly Save parameters.

If initialization fails, the failure is reported and the desktop must not present a
normal usable connected state.

## Connected capability presentation

The Device section presents authoritative session information returned by the
Application layer, such as:

- connected endpoint;
- FAST sample rate;
- NORMAL sample rate;
- FAST channel capacity;
- NORMAL channel capacity;
- FAST block size;
- other transport/device capabilities when they become part of the stable contract.

The page does not infer these values from firmware names or hard-coded board
assumptions.

The bottom status bar shows only compact connection/device identity. Detailed
transport/capability information remains on this page.

## Disconnect semantics

Disconnect is session teardown, not a synonym for the global Motor Stop button.

The Application may perform required workflow cancellation and motor-safe cleanup as
part of teardown, but that cleanup remains Application-owned.

A normal Disconnect must ensure that operations belonging to the old session cannot
continue acting after teardown, including:

- pending semantic motor commands;
- Tuning workers;
- Automation operations;
- user Scope acquisition;
- runtime acquisition workers;
- stale asynchronous frontend results.

The GUI reports Disconnected only after Application teardown succeeds.

If teardown fails or times out, the desktop must not silently discard the session
and pretend that disconnection succeeded. The failure remains visible and the
session remains recoverable where possible.

## Reconnect and generation boundaries

A new connection is a new session generation.

State tied to the previous device session must not become authoritative in the new
one. This includes stale:

- Parameter writes/readbacks;
- RuntimeTelemetry snapshots;
- Problems projections;
- Scope/Tuning snapshots;
- Automation operations;
- Action completions.

Host-only configuration that is explicitly designed to survive connection changes,
such as the editable Motion command model or connection form text, may persist.
Connection-specific execution state must reset.

## Busy and duplicate actions

Connect and Disconnect are serialized user operations.

Repeated clicks while one connection transition is in flight must not create
parallel sessions or duplicate teardown.

The UI must show a stable busy/disabled state rather than relying on timing to avoid
double execution.

## Error behavior

Connection failures remain explicit.

Examples include:

- endpoint unavailable;
- transport open failure;
- HostSchema load/validation failure;
- initial Parameter synchronization failure;
- capability discovery failure;
- teardown timeout/failure.

An error must not be converted into a successful connected/disconnected state merely
to keep the page responsive.

## Acceptance

The current Connection workflow is complete only when all of the following are
verified against the current desktop build and test firmware:

- endpoint refresh works without motor/application side effects;
- the selected endpoint and HostSchema path survive page navigation;
- the HostSchema path survives application restart when local persistence is
  available;
- Connect initializes exactly one usable ApplicationSession;
- the first visible Parameter state matches authoritative device RAM;
- displayed acquisition capabilities match the connected device;
- Connect does not energize or run the motor and does not create a Scope record;
- status-bar baseline telemetry becomes available after connection;
- Disconnect cleans up session-owned workflows and acquisition;
- reconnect cannot receive stale state/actions from the previous generation;
- failed Connect and failed Disconnect remain explicit and recoverable.

## Non-goals

The Connection page is not:

- a motor-control page;
- a Scope start/stop surface;
- a firmware flashing tool;
- a replacement for Problems/Events;
- a second owner of Parameter or runtime state.
