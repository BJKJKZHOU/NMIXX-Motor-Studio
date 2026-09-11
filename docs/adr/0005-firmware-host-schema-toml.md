# ADR 0005: Firmware Host schema is imported as TOML

- Status: Accepted
- Date: 2026-09-11

## Context

AxDr_L firmware already maintains `Parameter/parameter.yaml` as the single source of truth for Host-visible Parameters and Actions. NMIXX Motor Studio needs the same IDs, types, access rules, units and descriptions without duplicating firmware implementation details or maintaining a second hand-written table.

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

The first schema keeps the firmware symbols (`PARAM_MOTOR_RS`, `ACTION_*`) as stable identifiers. Human-facing names such as `motor.rs` are only exported when an explicit `host_name` is present in `parameter.yaml`; NMIXX does not infer public names from C symbols.

## Consequences

- `parameter.yaml` remains the only hand-maintained contract source.
- Firmware implementation names do not leak into normal NMIXX behavior except for the intentionally exported stable symbol.
- Schema files are readable and diff-friendly and may contain generation comments.
- NMIXX can resolve metadata by ID, symbol or explicit Host name.
- Future GUI forms, CLI names and external APIs can use the same metadata.
- Schema compatibility can later be checked using `schema_version`, protocol ID, source Git SHA and a content hash.
