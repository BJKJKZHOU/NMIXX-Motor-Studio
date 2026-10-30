# Parameters Page

> **Normative page contract.** This document defines the generic expert Parameter view. Shared Parameter ownership and edit semantics are defined by `SHARED_PARAMETER_STATE.md` and `UI_INTERACTION_RULES.md`.

## Purpose

Parameters is the expert view over the complete HostSchema Parameter registry.

It is not the owner of Parameter values. Opening this page must not be required for
Motor, Encoder, Limits, Control, Motion, Tuning or any other business page to work.

```text
Device RAM
    <->
Application ParameterService
    ->
shared frontend Parameter projection
    +-> Parameters page
    +-> business pages
```

The page is intentionally metadata-rich and may expose symbols/types that normal
business pages hide.

## Registry source

The registry and metadata come from the loaded HostSchema/Application contract.

The page does not maintain a hand-written Parameter list.

For each exposed Parameter, the expert view may present:

- ID;
- symbol;
- human-facing label;
- canonical current value;
- unit;
- access mode;
- wire/value type;
- valid range;
- write-state restriction;
- description/error details.

Missing metadata is shown as unavailable rather than guessed.

## Connection behavior

When disconnected, the page shows that no Parameter registry is available.

After Connect, it consumes the same already-populated shared Parameter state used by
the rest of the application. Merely opening Parameters must not cause another full
device read.

The global Read action is the explicit operation that refreshes active device RAM
into the shared Parameter state.

## Search and sorting

Search and column sorting are local expert-view operations.

They must not:

- read the device;
- write the device;
- reorder the underlying registry contract;
- change another page's Parameter state.

Search operates on the presented registry metadata/value text and only changes which
rows are visible.

Sorting only changes presentation order.

## Value presentation

Read-only Parameters are displayed as values and are never rendered as editable
device controls.

Writable Parameters use the same shared editor semantics as business pages:

```text
typing -> local draft
Enter  -> validate/write RAM/read back canonical value
Escape -> discard draft
blur   -> discard uncommitted draft
```

A failed write remains an error. It must not become the committed displayed value.

Discrete/enum values may use a suitable complete-value editor when one is provided,
but they still use the same shared Parameter write/readback path.

## Authoritative readback

The device/Application result is authoritative.

After a successful write:

1. the written Parameter is read back;
2. HostSchema-declared dependent Parameters are read back;
3. the Application cache is updated;
4. every page observes the same committed values.

The Parameters page must not calculate dependent gains, effective limits, encoder
results or other firmware-derived values itself.

## Write restrictions

Write permission and runtime write-state restrictions come from the authoritative
Parameter/Application contract.

The generic page does not bypass state restrictions merely because it is an expert
surface.

If firmware rejects a write in the current motor/application state, the rejection is
shown and the shared committed state remains authoritative.

## RAM-modified and persistence state

Persistent-eligibility comes from HostSchema metadata.

A writable persistent Parameter whose current RAM value differs from the current
session's persisted baseline uses the same RAM-modified presentation as all business
pages.

The Parameters page does not own Save.

Global Save persists firmware-defined persistent configuration and, only after
successful Save plus synchronization, establishes the new shared presentation
baseline.

RAM-only targets/commands are not marked modified merely because their value
changes.

## Errors

A Parameter row may show:

- read failure;
- write rejection;
- write/readback failure;
- schema/type/range validation failure.

An error in one row must not erase unrelated known Parameter values.

Recovery through a later successful read/write updates the same shared state rather
than creating a page-local replacement value.

## Expert symbols and labels

Unlike normal business pages, the Parameters table intentionally shows both the
human label and the stable programmatic symbol.

Normal pages continue to use HostSchema labels or workflow-specific product labels
and must not copy this expert presentation.

## Import/export scope

Architecture identifies import/export as a possible expert Parameter capability, but
no stable import/export file format, merge policy, validation policy or persistence
behavior is currently defined.

Therefore import/export is **not part of the current verification target**. It must
not be implemented ad hoc.

Before import/export becomes an accepted feature, its normative contract must define
at least:

- file format/versioning;
- whether values represent RAM, persisted configuration, or both;
- device/schema compatibility checks;
- treatment of read-only and unknown Parameters;
- validation and partial-failure behavior;
- whether import writes RAM only or may explicitly request Save;
- preview/confirmation behavior.

Once that contract exists, corresponding verification items must be added before
implementation is considered complete.

## Acceptance

The current Parameters page is complete only when:

- all HostSchema Parameters appear without requiring page-local discovery;
- opening the page causes no independent full device refresh;
- labels/symbols/types/access/ranges/write-state metadata are represented correctly;
- search filters presentation only;
- sorting changes presentation only;
- writable edits obey Enter/Escape/blur semantics;
- rejected writes return to authoritative shared state with a visible error;
- dependent readbacks update this page and every other consumer consistently;
- read-only Parameters cannot be edited;
- RAM-modified state agrees with business pages;
- global Read and Save update the same shared state and persistence baseline.

## Non-goals

For the current verification target, the page is not:

- a second Parameter cache;
- a firmware rule engine;
- an automatic Save surface;
- a raw protocol console;
- an import/export implementation without a separately approved contract.
