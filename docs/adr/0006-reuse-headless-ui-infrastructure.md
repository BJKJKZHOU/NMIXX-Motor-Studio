# ADR 0006: Reuse mature headless UI infrastructure

## Status

Accepted.

## Context

NMIXX Motor Studio needs a consistent engineering-tool interface, but generic UI interaction mechanisms such as data-table sorting/filtering, plotting interaction, terminal emulation, code editing and IDE-style docking are already mature problem domains.

Hand-writing those mechanisms makes pages larger, less consistent and harder to maintain, while adopting a full visual framework would let a third-party design system define the product appearance.

## Decision

NMIXX owns product semantics, information architecture and presentation style. Generic complex interaction behavior should be delegated to mature headless or low-presentation libraries where practical.

Current choices:

- VSCode Elements + Codicons: basic controls and icons.
- uPlot: Scope/FFT/Bode plotting mechanics.
- Split.js: simple split panes.
- TanStack Table: Parameters and Events table behavior.

Deferred until a concrete need exists:

- Dockview: draggable/persisted IDE-style workspaces.
- xterm.js: Automation terminal frontend.
- Monaco Editor: script editor.

Domain components continue to use the shared Application API and must not duplicate protocol/device behavior. Third-party libraries own generic interaction state, not motor-control policy or device state.

## Consequences

- NMIXX keeps a coherent visual identity while reducing custom UI machinery.
- Table and plotting features should be implemented by configuring the approved library before adding custom behavior.
- New large UI dependencies require an explicit product need and architecture review.
- Opinionated visual frameworks and generic dashboard templates are not the default path.
