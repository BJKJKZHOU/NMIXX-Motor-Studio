# Automation: application workflows, not a CLI wrapper

## Status and scope

This page contract implements the peer-client decision in `ARCHITECTURE.md` and
ADR 0001. The existing Automation placeholder is not an implementation. This
contract alone does not claim that a runner, editor, or device workflow is tested.

Automation replaces a sequence of manual application operations with a script.
It is not terminal-driven `nmixxctl` execution and is not limited to diagnostics.

```text
GUI actions ---------+
Automation scripts --+--> the current ApplicationSession --> domain services
CLI client ----------+                                      --> DeviceSession
```

The desktop runner uses the current connection and loaded schema. It does not
open a serial port, create another device session, synthesize protocol frames,
or repeat domain policy in the scripting client. A script calls the operation
represented by a GUI control; it does not locate that control by screen position.
Changing pages must not affect an executing workflow.

## Shared operations

- Parameter writes use the same write-and-dependent-readback service as Enter
  and selectors. Committed values appear in all Parameter views. Draft text is
  not silently committed, and Save is a separate explicit operation.
- Enable, Disable, motor Stop, Motion Run, identification and phase search use
  existing semantic Application methods. Ordinary Run does not implicitly Enable.
- Finite actions wait for their actual completion event. A successful command
  response is not universally an action-complete event. Save and identification
  Apply are synchronous at the current firmware boundary; ordinary Motion Run
  is acceptance, not arrival at a position target.
- Scope configuration, live recording, Stop and snapshots use shared acquisition.
  Scripts do not start a parallel stream or decoder. Tuning uses its existing
  independent experiment recording, not the Scope recording as scratch space.
- A script may compose loops and thresholds around those operations. A new test
  procedure must not require another core service or a new private protocol.

The API is language-neutral. Python can be the first supplied client and example
language; interpreter choice and IPC framing do not turn Automation into a CLI.
An interpreter is an execution tool, not the public motor-control API.

## Task lifecycle and cancellation

The initial desktop supports one active Automation task per connection. Task
state and logs are application-owned, not owned by the currently mounted page.
A task is associated with the original session; it never migrates to a reconnect.

Conflicting manual mutations must be rejected while an Automation workflow owns
those operations. Read-only views continue to update. Global motor Stop and
Disable remain available and revoke pending scripted starts before stopping.

Three operations have different meanings:

| Operation | Meaning |
| --- | --- |
| Scope Stop | Freeze the corresponding waveform record; do not stop the motor or baseline telemetry. |
| Motor Stop | Request the existing controlled stop; acceptance is not proof that motion has stopped. |
| Cancel Automation | Reject further script operations, end the script and clean up operations started by that task. |

Cancelling a read-only script must not stop unrelated motion or a pre-existing
Scope recording. If the script started motion, failure, cancellation or script
exit must not abandon it: request controlled Stop and report cleanup failure.
Do not silently substitute Disable for a failed controlled Stop. Stop latency
and completion must remain observable, including the existing 60-second limit.

Disconnect revokes script access to the old session and performs existing device
teardown. Killing an interpreter alone is not device cleanup. A stopped or failed
task cannot send a delayed Run after cancellation or reconnection.

## Desktop page

The Automation page provides script loading, execution, cancellation, progress,
output, error and exit status, and log export. A selected script is never run just
because the page was opened or a device connected. The same script contents shown
for the run must be the contents executed.

Script editing and terminal emulation are presentation tools, not the automation
architecture. Use the already approved Monaco/xterm infrastructure when those
interactive features are implemented; do not build a replacement editor or shell.
A plain read-only source preview and log is not to be described as an IDE/terminal.
Load page-only presentation dependencies lazily so Automation cannot prevent the
Connection workspace from mounting.

Run only trusted local scripts. A restricted Application API is not an operating
system sandbox: the selected interpreter still has the user's normal OS access.
No network listener or unauthenticated remote control endpoint is needed for an
application-launched script using its private process pipes.

## First useful workflows

A read-only diagnosis is one example, not the feature boundary. It compares a
cached value before Read, the actual device Read result and the cache afterward;
stream progress must be checked independently of whether numerical values change.
Exact `turn + rad` positions and f32 total-turn waveforms retain different types.

A motor workflow uses existing operations in order:

```text
commit configured parameters
  -> explicit Enable
  -> start or attach to recording
  -> Motion Run
  -> wait for the script's bounded condition
  -> motor Stop and wait for stopped state
  -> Scope Stop if this workflow started that record
  -> report the recorded result
```

Motion and identification examples must be explicitly selected and configured by
the user; the default example must not energize the motor. Parameter changes are
not automatically rolled back or saved to Flash when the script exits.

## Acceptance

- The same parameter write through GUI and Automation produces the same canonical
  value and dependent readbacks in every view, without opening another transport.
- Finite-action waits observe a monotonic deadline despite unrelated telemetry.
  Synchronous Save/Apply do not wait for a nonexistent completion notification.
- Cancel during preparation prevents a later Run. Cancel after Run requests motor
  cleanup. Read-only cancellation does not issue a motor/Scope command.
- Scope Stop freezes its record while telemetry continues. A Tuning script does
  not overwrite an existing frozen Scope record.
- Page switching retains the task/log. Disconnect prevents old tasks from acting
  on either the old or a newly connected device.
- Interpreter start failure, abnormal exit, excessive output, task timeout and
  cleanup failure produce explicit results; they are never reported as success.
- API/mock tests and frontend build checks are not substitutes for WebView and
  physical-device acceptance. Record separately which checks were actually run.
