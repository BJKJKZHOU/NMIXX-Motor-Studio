# Documentation policy

NMIXX documentation has different authority levels. A code change must not silently
rewrite a product or architecture rule merely to match the current implementation.

## Normative product and architecture documents

These documents define intended behavior and architectural boundaries:

- `ARCHITECTURE.md`
- `UI_INTERACTION_RULES.md`
- page interaction/design documents such as `SCOPE_PAGE.md`, `MOTION_PAGE.md`,
  `ENCODER_PAGE.md`, `MOTOR_PAGE.md`, `LIMITS_PAGE.md`, and
  `CONTROL_ARCHITECTURE_PAGE.md`

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

## Architecture Decision Records

Files under `docs/adr/` are historical decisions. Do not rewrite an accepted ADR
to make history look like the current design. If a decision changes, add a new ADR
that supersedes the old one and link the two.

## Architecture cleanup rule

Architecture cleanup may move code, remove duplicate implementations and narrow
internal APIs. It must not silently change established workflows, action semantics,
refresh behavior, safety behavior, or UI interaction rules. A desired behavior
change is a separate product decision, not a cleanup side effect.
