# Scope page interaction design

## Purpose

The Scope page is the general-purpose time-domain analysis surface in NMIXX Motor Studio. It should behave like a compact engineering oscilloscope for continuous acquisition and history inspection, without duplicating device acquisition state in the GUI.

This document defines the first oscilloscope-style interaction set. Triggering is intentionally excluded from the current Scope page milestone.

## Product boundary

Scope owns:

- channel selection and FAST/NORMAL acquisition rate selection;
- continuous Run/Stop acquisition;
- time-domain history viewing;
- per-channel vertical scale and position;
- mouse-centered horizontal zoom;
- horizontal history pan;
- draggable time cursors and cursor measurements;
- Auto Set for initial horizontal/vertical presentation.

Scope does not currently own:

- edge/level trigger configuration;
- Single-shot trigger capture;
- pulse/window/logic trigger modes;
- horizontal Y cursors;
- rectangle zoom;
- control-tuning experiment sequencing.

Trigger and Single-shot behavior must not be invented in the GUI before their acquisition semantics are deliberately added to the Application layer.

## One display state per concept

The page reuses one state for each display concept:

```text
timePerDiv          horizontal time scale
horizontalOffset    history position; 0 == Latest
verticalScale[id]   per-channel units/div
verticalOffset[id]  per-channel Y position/reference
cursorA / cursorB   X1 / X2 time cursor positions
```

Mouse interaction, numeric fields and plot rendering all edit these same values. A mouse gesture must not introduce a parallel pan/zoom/offset state.

## Channel controls

Every selected channel exposes its color identity and vertical controls directly in the channel selector:

```text
[x] ━ Iq                                    20K
    Scale/div [ 0.5 ] A/div       Y Pos [ 0.0 ] A
```

Unselected channels remain one row high and do not own an active waveform color.

A channel receives a waveform color when it is selected. Color assignment is stable for the current device session:

- adding or removing another channel must not recolor channels that remain selected;
- if a previously selected channel is enabled again and its previous color is still free, that color is reused;
- if the previous color is occupied, the first free palette color is assigned;
- the channel selector color mark, waveform and Y-position marker always use the same color.

The plot header does not duplicate channel legend entries or per-channel latest values. It is reserved for Scope-level information such as sample count and loss state.

Each visible waveform has an identically colored Y-position marker on the left edge of the plot. Dragging that marker changes only that channel's `verticalOffset`. The numeric `Y Pos` field and marker are two views over the same value.

`Scale/div` remains the only vertical gain control. It stores the real physical quantity per division and does not introduce a separate display-gain multiplier.

- values are stored internally in the channel's real unit;
- the UI formats small/large values with engineering prefixes such as mA, mrad and µrad;
- fast scale changes follow oscilloscope-style 1-2-5 steps;
- Auto Set rounds its calculated vertical scale upward to a 1-2-5 step;
- arbitrary positive numeric input remains allowed;
- scrolling the mouse wheel while hovering the channel's Y marker changes only that channel's `Scale/div`;
- scrolling over the plot background continues to change only `Time/div`.

The Y marker and Scale/div controls do not change acquisition configuration and must not modify sample values.

## Horizontal navigation

### Pan

Dragging the plot background horizontally moves through Scope history.

```text
drag right  -> older history -> horizontalOffset increases
drag left   -> newer history -> horizontalOffset decreases
```

`horizontalOffset == 0` means the latest acquired window and therefore **Follow Latest** is active. While acquisition is running, new samples stay at the right edge.

Dragging into history makes `horizontalOffset > 0` and leaves Follow Latest without stopping acquisition. The existing horizontal Position control remains a secondary precise/navigation control. A `Latest` action returns `horizontalOffset` to zero and re-enters Follow Latest.

Panning changes only the viewed time range. It does not Stop acquisition.

### Wheel zoom

The mouse wheel changes `Time/div` through the existing oscilloscope-style 1-2-5 steps.

Zoom is anchored to the time under the mouse pointer. The time under the pointer should remain at approximately the same screen X coordinate after the scale step unless clamping at the available history boundary makes that impossible.

The wheel does not modify Y scale in this milestone.

## Pointer interaction priority

Pointer hit testing follows one fixed priority:

```text
pointer down
    |
    +-- hit X1 / X2 line or marker -> drag that cursor
    |
    +-- hit channel Y marker ------> drag that channel Y position
    |
    +-- otherwise -----------------> horizontal history pan
```

The priority is independent of Cursor mode for background panning. Enabling cursors must not disable normal history navigation.

## Time cursors

Cursor mode owns two vertical time cursors, X1 and X2.

When Cursor is enabled and no existing cursor positions are available, X1 and X2 are initialized near 30% and 70% of the visible time window. The old "click once for X1, click again for X2" placement model is not used.

Each cursor is rendered as:

- one vertical line through the plot;
- one marker at the top edge;
- one same-colored marker at the bottom edge;
- an X1/X2 identity.

The line and both markers are draggable hit targets. Dragging clamps the cursor to the visible time window.

A compact label between X1 and X2 shows `Δt · 1/Δt`. That label is also a drag handle: dragging it moves X1 and X2 together while preserving their interval, clamped to the visible time window.

The Cursor panel shows:

- X1;
- X2;
- Δt;
- 1/Δt;
- for every visible channel: value at X1, value at X2 and ΔY.

The first implementation may use the nearest captured sample for channel cursor values; interpolation is not required.

## Hover time marker

Moving the pointer across the plot without dragging shows a lightweight temporary time marker at the pointer's X position.

The hover marker:

- shows time only;
- disappears when the pointer leaves the plot;
- does not create or move X1/X2;
- does not become persisted Scope state;
- exists only for quick visual inspection.

## Run / Stop and frozen data

`Stop` stops Scope acquisition but preserves the completed waveform and current view state.

After Stop, the user must still be able to:

- pan and zoom time;
- change per-channel Scale/div and Y Pos;
- drag Y markers;
- enable/drag cursors;
- inspect cursor values and hover time;
- use later measurement/export functions.

Stop must not clear captured samples, reset channels, reset vertical scales or discard cursors.

Starting a new Run returns the time view to Latest/Follow Latest. It does not require resetting the user's saved channel layout or vertical configuration.

## View settings persistence

Scope persists **view settings**, not acquisition data.

The persisted view includes:

- selected channel IDs that still exist on the current device/schema;
- FAST/NORMAL rate selection;
- stable channel color assignment;
- per-channel Scale/div;
- per-channel Y Pos;
- Time/div;
- Cursor enabled/disabled preference;
- active channel where it still exists.

The persistence key is derived from the current channel/capability signature so unrelated firmware/device layouts do not blindly receive the same view.

The following are deliberately not restored:

- captured samples;
- Scope LIVE/STOPPED runtime state;
- lost-frame counters;
- historical `horizontalOffset`;
- previous absolute X1/X2 positions.

When Cursor is restored as enabled, X1/X2 are initialized in the new visible window rather than reusing old acquisition times.

## Interaction and acquisition traffic

Pointer movement must stay local and responsive.

- Y-marker drag updates the local uPlot scale directly.
- X-cursor drag redraws cursor graphics locally.
- horizontal drag updates the local X scale while moving.
- wheel zoom updates the local X scale immediately.

History snapshot reads are deferred/debounced rather than issued for every pointer-move event. A completed pan/zoom then refreshes the corresponding history window from the existing Scope API.

No new transport path, Plot protocol message or device-side state is introduced for these interactions.

## Visual rules

- Channel selector marks, waveforms and Y markers share one per-channel color identity.
- Channel colors are allocated only to selected channels and remain stable while other channels are added or removed.
- X1 top/bottom markers use one consistent cursor color; X2 uses a second consistent cursor color.
- Cursor colors are not channel colors and do not imply a channel association.
- Markers are small but have a larger invisible hit tolerance than their painted size.
- Plot background uses a grab/grabbing cursor for history pan.
- Hovering a Y marker indicates vertical dragging.
- Hovering an X cursor indicates horizontal dragging.

## Current non-goals

The following are deliberately deferred:

- Trigger;
- Single;
- touchpad-specific gestures;
- inertial scrolling;
- rectangle zoom;
- horizontal Y cursors;
- acquire-vs-hide channel separation.

These may be added when their workflow is required, without changing the state ownership rules above.
