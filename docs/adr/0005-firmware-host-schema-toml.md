# ADR 0005: Firmware Host schema is imported as TOML

- Status: Accepted
- Date: 2026-09-11

## Context

AxDr_L firmware already maintains `Parameter/parameter.yaml` as the single source of truth for Host-visible Parameters and Actions. NMIXX Motor Studio needs the same IDs, types, access rules, units, labels and descriptions without duplicating firmware implementation details or maintaining a second hand-written table.

The source YAML also contains firmware-only fields such as `binding`, `getter`, `command` and `on_change`. Those must not become part of the Host API contract.

## Decision

Firmware exports a deterministic Host-facing TOML manifest from `parameter.yaml`.

```text
Parameter/parameter.yaml
        |
        | firmware exporter
        v
axdr-host-schema.toml
        |
        | NMIXX parser
        v
HostSchema / ParameterMetadata / ActionMetadata
```

The TOML file is generated and must not be hand-edited. NMIXX treats it as an import/serialization format only; clients consume Rust metadata types rather than TOML parser values.

Each exported Parameter/Action has two different naming roles:

```text
symbol
    stable machine-facing identity
    e.g. PARAM_MOTOR_RS / ACTION_MOTOR_ENABLE

label
    human-readable presentation text
    e.g. Rs / Enable
```

The stable `symbol` remains suitable for code, compatibility and lookup. `label` is presentation metadata and may be shown by GUI/CLI clients. A human-readable label must not replace or mutate the stable symbol.

NMIXX may resolve metadata by ID, stable symbol or label for presentation/convenience, but internal compatibility must continue to work when labels are introduced or refined.

NMIXX does not infer a separate public API name from C symbols. If a future external scripting/API naming scheme needs stable names distinct from firmware symbols, that must be introduced deliberately as a separate contract rather than overloading `label`.

## Consequences

- `parameter.yaml` remains the only hand-maintained contract source.
- Firmware implementation-only fields do not leak into the Host contract.
- Stable exported symbols remain available even when human-facing labels are added.
- GUI and other clients can present concise labels without using them as the only identity.
- Schema files are readable and diff-friendly and may contain generation comments.
- NMIXX can resolve metadata by ID, symbol or label where appropriate.
- Future GUI forms, CLI presentation and external APIs can consume the same metadata while keeping identity and presentation separate.
- Schema compatibility can later be checked using `schema_version`, protocol ID, source Git SHA and a content hash.
