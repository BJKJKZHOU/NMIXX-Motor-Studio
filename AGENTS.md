# Firmware adaptation

- NMIXX Motor Studio primarily targets the current AxDr_L_Motor firmware. The
  firmware defines the contract; the host follows it as it evolves.
- Before changing device-facing behavior, inspect the current firmware in the
  sibling `../AxDr_L_Motor` checkout (or the location supplied by the user),
  especially `Parameter/parameter.yaml`, `tools/export_host_schema.py`, and the
  relevant protocol/control implementation.
- Follow the firmware's actual IDs, types, enums, write-state restrictions,
  readback dependencies, capabilities, and Action response/completion semantics.
  Do not invent fields or values to satisfy obsolete host assumptions.
- Fix host/firmware mismatches in the host unless a firmware change is explicitly
  part of the task. Do not hand-edit generated HostSchema files.
- Adaptation to current firmware is the normal implementation path. Compatibility
  with older firmware is a separate, explicitly scoped concern and must not
  constrain current-firmware support.
- Keep test fixtures aligned with the current firmware export, and validate
  device-facing changes against that export as well as isolated unit tests.
