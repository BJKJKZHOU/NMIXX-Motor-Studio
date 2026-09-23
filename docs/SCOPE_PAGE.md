# Scope page interaction design

## Purpose

The Scope page is the general-purpose time-domain analysis surface in NMIXX Motor Studio. It behaves as a compact engineering waveform viewer for continuous acquisition and history inspection.

Triggering is intentionally excluded from the current Scope milestone.

## Product boundary

Scope owns:

- channel selection and FAST/NORMAL acquisition rate selection;
- continuous Run/Stop waveform recording;
- time-domain history viewing;
- per-channel vertical scale and position;
- continuous horizontal zoom and history pan;
- Follow Latest / Latest navigation;
- time cursors and cursor measurements;
- vertical Auto Set.

Scope does not currently own:

- edge/level trigger configuration;
- Single-shot trigger capture;
- pulse/window/logic trigger modes;
- control-tuning experiment sequencing.

Trigger and Single-shot behavior must not be invented in the GUI before their acquisition semantics exist in the Application layer.

## Shared acquisition and trace visibility

The Application connection owns one physical acquisition stream, not this page.
It continuously acquires the available baseline Iq, mechanical speed, position and
Vbus NORMAL channels and retains ten seconds of recent history. A baseline trace
is already a device channel before it is visible. Selecting/hiding it at its
baseline rate changes only the view; all traces may be hidden without stopping
runtime feedback. Existing history is available when a trace is revealed.

Only the merged baseline + live Scope + active Tuning demand is sent to firmware.
Duplicate parameter IDs are transmitted once; a required FAST source serves a
NORMAL consumer by integer decimation. Device limits apply to this merged demand,
including hidden baseline channels, not merely the checkbox count. Selecting a new
non-baseline signal or changing its sample rate can change device configuration.

Scope and Control Tuning use the same decoded samples but own separate recording
buffers. They must not replace each other's channel settings or frozen samples.
See `SHARED_ACQUISITION.md` for precision, rate and validation boundaries.

## Plot infrastructure boundary

ECharts owns generic plotting and interaction mechanics:

- waveform rendering;
- continuous horizontal zoom;
- horizontal pan;
- coordinate transforms;
- axis pointer / hover;
- multiple Y axes;
- resize and plot lifecycle.

NMIXX owns product semantics:

- acquisition Run/Stop;
- FAST/NORMAL capability and channel limits;
- Scope history;
- snapshot requests;
- Follow Latest;
- per-channel physical Scale/div and Y Pos;
- units, colors and page layout.

NMIXX must not reimplement ECharts pan/zoom coordinate math or maintain a parallel horizontal timebase model.

## Display state

The page owns one state for each product concept:

```text
viewRange             visible horizontal [start, end] time range
followLatest          whether the visible range tracks newest data
verticalScale[id]     per-channel physical units/div
verticalOffset[id]    per-channel Y position/reference
cursorA / cursorB     X1 / X2 time cursor positions
```

There is no horizontal `Time/div` state and no 1-2-5 horizontal timebase. Horizontal scale is the continuous ECharts dataZoom range.

## Channel controls

Every selected channel exposes its color identity, acquisition rate and vertical controls.

```text
[x] ● Iq                                  FAST
    Scale/div [ 0.5 ] A/div    Y Pos [ 0.0 ] A
```

Per-channel vertical scale remains a physical engineering quantity:

- `Scale/div` stores real units per division;
- `Y Pos` stores the channel's vertical center/reference;
- arbitrary positive Scale/div values are valid;
- Auto may round its calculated Scale/div to a convenient 1-2-5 engineering value;
- changing vertical view state never changes acquisition data.

Each selected channel has its own ECharts Y axis internally. Only the active channel's Y-axis labels need to be visible.

## Horizontal navigation

### Continuous zoom

Horizontal zoom is owned directly by ECharts `dataZoom`.

Mouse wheel / trackpad zoom changes the visible time span continuously. NMIXX does not quantize or snap the result to oscilloscope-style timebase steps.

The time under the pointer and the exact zoom behavior follow the ECharts interaction model.

### Pan

Dragging the plot moves through the available Scope history using ECharts pan/dataZoom behavior.

Pan changes only the visible range. Acquisition continues while browsing history.

### Follow Latest

A new Run starts in Follow Latest using the default initial span of 0.5 s.

While Follow Latest is active:

- the visible range remains attached to the newest data;
- the current visible span is preserved;
- new data appears at the right edge.

Any user horizontal zoom or pan leaves Follow Latest.

The **Latest** action returns the current visible span to the newest data without resetting that span.

Example:

```text
history view: [-4.183, -4.000]  span = 0.183 s
Latest
latest view:  [-0.183,  0.000]  span = 0.183 s
```

Only a new Run resets the horizontal span to the default 0.5 s.

## Acquisition requests from the viewport

ECharts is the source of truth for the horizontal viewport.

When a history refresh is needed, NMIXX converts the visible range to the existing Scope Application API:

```text
visibleRange = [start, end]

windowSeconds    = end - start
endOffsetSeconds = max(0, -end)
```

Pan/zoom pointer movement remains local inside ECharts. Snapshot reads are debounced and occur after view changes rather than for every pointer-move event.

No new transport path or device-side Plot state is introduced for horizontal navigation.

## Run / Stop and frozen data

`Stop` stops appending to this Scope recording and preserves captured history and
the current display state. It does not issue Motor Stop. Baseline acquisition and
shared Parameter runtime values continue, as does an independently active Tuning
recording. Channels needed only by this stopped record can be released.

The stopped record owns retained samples, not offsets into the rolling baseline
buffer. Later telemetry, Tuning runs and baseline buffer wrap cannot overwrite it.
Changing visibility while stopped can reveal only data present in that frozen
record; it must not substitute newer baseline history. A new Run starts a new
Scope record seeded with available recent baseline data.

After Stop the user must still be able to:

- pan and zoom history;
- return to Latest;
- change per-channel Scale/div and Y Pos;
- inspect hover/cursor values;
- use later measurement/export functions.

Stop must not clear captured samples or reset channel/vertical settings.

Starting a new Run returns to Follow Latest with the default 0.5 s span.

## Time cursors

Cursor mode owns X1 and X2 time cursors.

When enabled without existing cursor positions, X1 and X2 initialize near 30% and 70% of the current visible range.

The Cursor panel may show:

- X1;
- X2;
- Δt;
- 1/Δt;
- per-visible-channel value at X1, value at X2 and ΔY.

The first implementation may use nearest captured samples; interpolation is not required.

Cursor interaction should use ECharts/ZRender extension points rather than a second general-purpose plot interaction system.

## View settings persistence

Scope persists view configuration, not acquisition history.

Persist:

- selected channel IDs that still exist;
- FAST/NORMAL selection;
- stable channel colors;
- per-channel Scale/div;
- per-channel Y Pos;
- Cursor enabled/disabled preference;
- active channel.

Do not persist:

- captured samples;
- LIVE/STOPPED state;
- lost-frame counters;
- historical horizontal position;
- absolute cursor timestamps;
- horizontal zoom span for the initial implementation.

A new Run therefore always starts at Latest with the default 0.5 s span.

## Display data and downsampling

The Application layer keeps the high-resolution Scope history.

The desktop snapshot path limits data sent to the chart according to the requested viewport and display point budget while preserving extrema.

ECharts must not apply a second lossy waveform sampler on top of the Host-prepared data. Scope line series therefore use no ECharts LTTB/average sampling.

## Visual rules

- Channel selector marks and waveforms share one stable per-channel color.
- The active channel's Y axis uses that channel color.
- Cursor colors are independent from channel colors.
- The plot keeps NMIXX's VS Code-like engineering-tool visual style.
- Generic interaction mechanics remain owned by ECharts.

## Current non-goals

The following are deliberately deferred:

- Trigger;
- Single;
- inertial scrolling policy beyond the plotting library's normal behavior;
- horizontal Y cursors.

These may be added when their workflows are required without reintroducing a parallel horizontal timebase model.
