# Events / Problems Page

## Purpose

The Events / Problems page is the detailed diagnostic surface for active problems and retained event history.

The top-right Problems indicator only summarizes the most important active problem and the total active count. This page shows each problem individually and explains what it means and where the user should normally go to resolve it.

## Active problems

Active problems use a three-column layout:

```text
Level / Hint        Detailed description                 Quick action
--------------------------------------------------------------------------
Error · Encoder     Feedback is unavailable. Explain     [ Open Encoder ]
                    likely causes, effect and checks.

Warning · Limits    Required operating configuration     [ Open Limits ]
                    is missing or unsuitable.
```

### Left column: Level / Hint

Keep this concise.

It contains:

- severity;
- a short domain/problem hint.

Examples:

```text
Error · Encoder
Fault · Motor
Warning · Identification
Warning · Limits
Info · Connection
```

Do not expose low-level implementation-module names here unless they are genuinely the user-facing concept.

### Middle column: Detailed description

This is the primary diagnostic content.

It should explain, when known:

- what happened;
- why the condition can occur;
- what functionality is affected;
- what the user should normally inspect, configure or retry.

Technical details such as codes, raw device status, timestamps, source and occurrence count may be shown in an expandable detail area rather than crowding the default row.

### Right column: Quick action

The right column provides a shortcut to the page where the issue is normally resolved, for example:

```text
Encoder problem        -> Open Encoder
Motor parameter issue -> Open Motor
Limit issue            -> Open Limits / Safety
Control issue          -> Open Control
Identification issue   -> Open Motor
Analysis issue         -> Open Analysis
Connection issue       -> Open Connection
```

The shortcut is navigation, not the owner of the condition. Opening the destination page does not itself mark the problem solved.

## Problem lifecycle

Active state and history retention are separate concepts.

```text
problem appears
    -> Active
    -> History record created/updated

condition later passes
    -> removed from Active
    -> History record remains as Resolved

user explicitly clears history
    -> retained history record may be deleted
```

Merely viewing a problem does not clear it.

## Refresh / recheck

The page and top-right Problems control may expose Refresh.

Refresh re-evaluates current conditions through the Application layer and device state.

```text
Refresh
    |
    +-- still failing -> remain Active
    |
    +-- check passes -> remove from Active, retain History
```

This operation is intended for cases where the user has corrected wiring, configuration, limits, encoder settings or another external condition and wants the application to verify that the problem is gone immediately.

Refresh must not be implemented as "hide the warning". Resolution follows the current authoritative condition.

## Device fault clearing is separate

Some firmware faults may require an explicit device-side clear/reset action.

When such an operation exists, it is separate from Refresh and separate from history deletion:

```text
Refresh
    re-evaluate state

Clear Fault
    request device fault clear/reset semantics

Clear History
    remove retained resolved records
```

A successful device fault clear may cause a subsequent/current recheck to resolve the Active problem, but the corresponding history remains.

## History

History records events and problems even after they stop being active.

Useful fields include:

- severity;
- domain/hint;
- concise summary;
- first occurrence time;
- last occurrence time;
- occurrence count;
- Active / Resolved state;
- source/code when available.

Resolved entries are not automatically deleted. They remain until the user explicitly invokes Clear History.

Active problems must not disappear merely because history was cleared; current state can always repopulate/recreate the Active view from authoritative runtime/device conditions.

## Top-toolbar relationship

The top-right Problems indicator is deliberately compact:

```text
[ severity · most-important-problem  active-count ]
```

For example:

```text
[ Error · Encoder problem  3 ]
```

The badge `3` means there are three currently active problems. The text names only the single highest-priority active problem.

Clicking the indicator opens this page. Hover, if provided, remains summary-level and does not duplicate this page's diagnostics.

## Application data model

The Application layer should expose structured problem/event information rather than forcing the GUI to parse strings.

A practical conceptual model is:

```text
Problem
    id
    severity
    domain
    summary
    description
    cause/help text
    suggested_domain
    source/code
    active
    first_seen
    last_seen
    occurrence_count
```

The exact Rust/API representation may evolve. The important requirement is that severity, presentation domain, lifecycle and navigation target remain structured data.
