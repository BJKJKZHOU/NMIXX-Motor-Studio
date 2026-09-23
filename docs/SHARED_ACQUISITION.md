# Shared runtime acquisition

## Data ownership

```text
DeviceSession -> MixedScopeSession (one layout-aware decode)
                     -> SharedAcquisition
                           -> baseline recent history (10 seconds)
                           -> ParameterService latest runtime values
                           -> Scope rolling / frozen recording
                           -> Control Tuning finite / frozen recording
```

`ApplicationSession::runtime_start` starts acquisition when a desktop connection
has completed its initial parameter read and capability discovery. It never enables
or runs the motor. GUI navigation does not own this connection-scoped stream.

Baseline signals are the available NORMAL-capable `PARAM_RUN_IQ`, `PARAM_RUN_WM`,
`PARAM_RUN_POSITION` and `PARAM_ADC_VBUS`. The current AxDr NORMAL rate is 1 kHz;
rates and channel limits are read from the device capabilities, not a UI timer.
Only this small baseline is always acquired, not the entire parameter dictionary.

`ParameterService` consumes decoded read-only f32 baseline samples without sending
Read requests. Its cache follows incoming batches; change notifications coalesce
to at most 60 Hz. A slower Read response cannot overwrite a newer stream value.
Configuration writes, dependent readback, Action readback and explicit Read retain
their existing paths. No new configuration-parameter polling is introduced.

## Wire precision boundary

The existing position Plot channel sends f32 total turns, while its Parameter value
is an i32 turn count plus an f32 within-turn angle. Reconstructing the latter from
the former would lose information. This host-only change does not pretend otherwise:
position history uses the original Plot representation, while exact position and
non-streamed Motor State keep the existing connection-level 500 ms reads.

Iq, mechanical speed and Vbus no longer have duplicate display polling. Limits has
no private Vbus poll; every page sees the same streamed Parameter value. FAST data
is dequantized using the device's advertised scale. A NORMAL consumer of a FAST
source receives decimated data, not invented/interpolated samples. Delivering exact
position at 1 kHz requires a future typed firmware stream contract, not a cast.

## Channel demand versus view selection

The actual device demand is the sorted union of baseline, live Scope and active
Tuning channels. The same parameter ID is sent once. FAST wins when one consumer
requires it; slower consumers decimate when rates are integer multiples. Firmware
FAST/NORMAL limits are validated against the union before reconfiguration.

A Scope selection at an already acquired baseline rate changes its visible channel
list only: no new Plot Config, Start or Stop, and no erased history. Hidden baseline
channels remain stored so later visibility can reveal existing samples. An empty
Scope selection is valid. New non-baseline channels and actual rate changes may
change the transport demand. Source-layout generations still belong to the existing
MixedScope decoder; no second transport or parallel protocol decoder is created.

## Stop and record lifetime

Scope Stop freezes its own buffers and loss count. It does not stop the baseline,
Tuning, or the motor. Hiding a trace is not stopping a recording. Editing selection
while stopped never replaces its samples with live data; Run creates the next
record from the selected layout and available baseline history.

Tuning captures directly from the same decoded batches into its own finite buffers.
Its configuration, status, duration and window APIs refer to that record, never to
the currently selected Scope view. Starting another Scope run cannot overwrite a
completed experiment. Tuning's existing motor-stop/failed-experiment cleanup remains
separate from stopping its recording. Disconnect shuts down the physical stream
after the established motor/task cleanup.

Baseline history is bounded at ten seconds; each view's recording is bounded at
128 MiB. The transport decoder retains only a 1 ms scratch ring in this shared
mode. Standalone MixedScope consumers retain their existing history behavior.
No history is automatically saved to disk. Stream errors invalidate streamed
Parameter values; this change does not claim loss-free or hard-real-time USB delivery.

## Regression checks

With repository dependencies installed:

```sh
cargo test -p nmixx-app
cd apps/desktop
node --test tests/runtime-view.test.cjs
npm run test:parameters
npm run test:plots
npm run build
```

Rust tests exercise union/deduplication, integer decimation, bounded baseline
history and independently frozen records. A scripted transport test uses the actual
MixedScope decoder and SharedAcquisition: baseline visibility and Scope/Tuning Stop
must leave the initial Config/Start count unchanged, while changing a stopped
view to a new signal reconfigures only when Run resumes. Plot position must not
populate a typed Position cache value.

The Node tests execute the actual Scope component callbacks with IPC mocks. They
are not a WebView integration test and do not operate hardware. Validate a real
connection separately: leave Scope unopened, show/hide baseline traces, stop Scope
while observing runtime values, run Tuning and reopen the stopped Scope, then wrap
the baseline buffer and inspect both frozen recordings. Also verify FAST/NORMAL
transitions, channel capacity rejection and disconnect. No motor Run is authorized
merely by connection or trace visibility.
