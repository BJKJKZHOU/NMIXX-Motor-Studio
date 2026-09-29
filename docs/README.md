# Documentation policy

NMIXX documentation has different authority levels. A code change must not silently
rewrite a product or architecture rule merely to match the current implementation.

## Normative product and architecture documents

These documents define intended behavior and architectural boundaries:

- `ARCHITECTURE.md`
- `UI_INTERACTION_RULES.md`
- `CONNECTION_PAGE.md`
- `MOTOR_PAGE.md`
- `ENCODER_PAGE.md`
- `LIMITS_PAGE.md`
- `CONTROL_ARCHITECTURE_PAGE.md`
- `CONTROL_TUNING_PAGE.md`
- `MOTION_PAGE.md`
- `SCOPE_PAGE.md`
- `PARAMETERS_PAGE.md`
- `EVENTS_PAGE.md`
- `AUTOMATION_PAGE.md`

Treat their design rules as requirements. If current code disagrees, normally fix
the code. Change a normative rule only after an explicit product/architecture
decision, and update all affected documents/tests together.

Sections explicitly named `Current implementation`, `Current GUI state`,
`Current firmware gaps` or similar are implementation-status notes inside an
otherwise normative document. Those status sections may be updated as the
implementation catches up.

## Implementation-contract documents

Documents such as `SHARED_ACQUISITION.md` describe the current internal mechanism
used to satisfy normative behavior. They may evolve during refactoring, but must
preserve externally visible behavior and the architectural invariants above.

## Integration execution/status documents

`INTEGRATION_VERIFICATION_MATRIX.md` is the current end-to-end verification
checklist. It is intentionally mutable operational state: checkboxes, test results,
hardware observations and issue records should be updated as integration proceeds.

It does not override normative design. If a verification item exposes a mismatch,
first identify the owning layer and fix the implementation. Do not weaken or rewrite
a normative architecture/page rule merely to make the checklist pass.

## Specification-driven verification workflow

Integration work starts from the normative documents, not from whichever symptom is
noticed first in the running GUI.

Use this order for every domain:

```text
normative contract
    ->
verification-matrix coverage
    ->
code/build/static checks
    ->
WebView/device execution
    ->
record mismatch
    ->
identify semantic owner
    ->
fix owner
    ->
rerun affected consumers/regressions
    ->
update verification status
```

Before testing a domain, first verify that every current-scope normative requirement
has a corresponding item in `INTEGRATION_VERIFICATION_MATRIX.md`. If a requirement
has no verification item, the matrix is incomplete; add the verification item before
claiming completion.

Likewise, a matrix item should be traceable to a normative product/architecture
requirement, an implementation-contract invariant, or an explicit regression risk.
Do not turn an incidental current implementation detail into a product requirement
merely because it is easy to test.

When a test fails, record the expected behavior from the normative document and the
observed behavior before changing code. Fix the layer that owns the semantic. Do not
default to patching the page where the symptom happened.

A shared-layer fix must be reverified on every affected consumer. For example, a
Parameter synchronization fix is not complete after the Parameters page works; Motor,
Encoder, Limits, Control, Motion and Tuning consumers affected by the same state must
be checked.

### Completion and scope

"Software completion" is measured only against an explicit current acceptance scope.

A future feature mentioned by the architecture does not silently count as an
unfinished current feature. It must be one of:

- **current scope** — normative semantics exist and the verification matrix contains
  acceptance items;
- **capability-gated** — the current product must correctly show unavailable/disabled
  behavior when firmware support is absent, while functional verification begins
  when that capability exists;
- **deferred/out of scope** — explicitly named as such, with no claim that it is
  implemented or verified.

If a feature is neither specified nor explicitly deferred, documentation coverage is
incomplete and the overall completion percentage must not hide that gap.

A domain reaches completion only when its matrix exit criteria are satisfied:
`Wired`, `Verified`, and `Regression Guarded` as applicable. A code review or
unit test alone cannot substitute for physical-device verification where the
contract depends on the controller.

## Architecture Decision Records

Files under `docs/adr/` are historical decisions. Do not rewrite an accepted ADR
to make history look like the current design. If a decision changes, add a new ADR
that supersedes the old one and link the two.

## Architecture cleanup rule

Architecture cleanup may move code, remove duplicate implementations and narrow
internal APIs. It must not silently change established workflows, action semantics,
refresh behavior, safety behavior, or UI interaction rules. A desired behavior
change is a separate product decision, not a cleanup side effect.
