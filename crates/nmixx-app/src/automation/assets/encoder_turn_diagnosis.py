"""Read-only hand-turn encoder diagnosis.

Precondition: motor must be DISABLED. During the 2 second observation window,
rotate the shaft by hand. This script does not write parameters, change Scope
configuration, Enable, Run, Stop, Disable or Save.
"""
import json
import math

from nmixx import client


def show(label, value):
    print("\n=== " + label + " ===", flush=True)
    print(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False), flush=True)


registry = client.parameters()
metadata = {item["symbol"]: item for item in registry}


def enum_value(symbol, wanted):
    meta = metadata[symbol]
    symbols = meta.get("allowedSymbols", [])
    if wanted not in symbols:
        raise RuntimeError(f"{symbol} does not expose enum value {wanted}")
    index = symbols.index(wanted)
    allowed = meta.get("allowed", [])
    return allowed[index] if index < len(allowed) else index


def rows_by_symbol(rows):
    return {row["symbol"]: row for row in rows if row.get("symbol")}


def typed_value(row, expected):
    if row is None:
        raise RuntimeError(f"Missing diagnostic row for {expected}")
    if row.get("error"):
        raise RuntimeError(f"{row.get('symbol')}: {row['error']}")
    value = row.get("value")
    if not isinstance(value, dict) or value.get("type") != expected:
        raise RuntimeError(f"{row.get('symbol')} has unexpected type: {value}")
    return value["value"]


def position_turns(row):
    value = typed_value(row, "position")
    return float(value["turns"]) + float(value["theta"]) / math.tau


required = [
    "PARAM_MOTOR_STATE",
    "PARAM_RUN_POSITION",
    "PARAM_ENCODER_WM",
    "PARAM_ENCODER_READY",
    "PARAM_ENCODER_VALID",
    "PARAM_ENCODER_FAULT",
]
missing = [symbol for symbol in required if symbol not in metadata]
if missing:
    raise RuntimeError("Loaded schema is missing: " + ", ".join(missing))

optional = ["PARAM_ENCODER_FAULT_REASON", "PARAM_CAL_VALID", "PARAM_RUN_WM"]
keys = required + [symbol for symbol in optional if symbol in metadata]

before_rows = client.read(keys)
before = rows_by_symbol(before_rows)
disabled = enum_value("PARAM_MOTOR_STATE", "DISABLED")
state = typed_value(before["PARAM_MOTOR_STATE"], "u8")
if state != disabled:
    raise RuntimeError(
        f"Disable the motor before hand-turn diagnosis (current state value {state}, DISABLED {disabled})."
    )

show("Before hand turn", before_rows)
progress_before = client.stream_progress()

print(
    "\nRotate the motor shaft by hand NOW. Observing exact Position and Encoder WM for 2 seconds...",
    flush=True,
)

samples = []
for index in range(10):
    client.wait(0.2)
    rows = rows_by_symbol(client.read(["PARAM_RUN_POSITION", "PARAM_ENCODER_WM"]))
    samples.append({
        "tSeconds": round((index + 1) * 0.2, 1),
        "positionTurns": position_turns(rows["PARAM_RUN_POSITION"]),
        "encoderWm": float(typed_value(rows["PARAM_ENCODER_WM"], "f32")),
    })

after_rows = client.read(keys)
after = rows_by_symbol(after_rows)
progress_after = client.stream_progress()
scope = client.scope_summary(window_seconds=2.0)

start_position = position_turns(before["PARAM_RUN_POSITION"])
end_position = position_turns(after["PARAM_RUN_POSITION"])
positions = [start_position] + [sample["positionTurns"] for sample in samples] + [end_position]
speeds = [sample["encoderWm"] for sample in samples]

before_progress = {item["id"]: item for item in progress_before["channels"]}
batch_delta = []
for item in progress_after["channels"]:
    previous = before_progress.get(item["id"])
    batch_delta.append({
        "id": item["id"],
        "symbol": item["symbol"],
        "receivedBatchDelta": None if previous is None else item["receivedBatches"] - previous["receivedBatches"],
        "ageMs": item["ageMs"],
    })

position_id = metadata["PARAM_RUN_POSITION"]["id"]
scope_position = next((series for series in scope["series"] if series["id"] == position_id), None)

summary = {
    "exactPositionStartTurns": start_position,
    "exactPositionEndTurns": end_position,
    "exactPositionDeltaTurns": end_position - start_position,
    "exactPositionObservedMinTurns": min(positions),
    "exactPositionObservedMaxTurns": max(positions),
    "encoderWmPeakAbsRadPerSec": max((abs(speed) for speed in speeds), default=0.0),
    "encoderReadyEnd": typed_value(after["PARAM_ENCODER_READY"], "u8"),
    "encoderValidEnd": typed_value(after["PARAM_ENCODER_VALID"], "u8"),
    "encoderFaultEnd": typed_value(after["PARAM_ENCODER_FAULT"], "u8"),
    "encoderFaultReasonEnd": (
        typed_value(after["PARAM_ENCODER_FAULT_REASON"], "u8")
        if "PARAM_ENCODER_FAULT_REASON" in after
        else None
    ),
    "calibrationValidEnd": (
        typed_value(after["PARAM_CAL_VALID"], "u8")
        if "PARAM_CAL_VALID" in after
        else None
    ),
    "runWmEnd": (
        typed_value(after["PARAM_RUN_WM"], "f32")
        if "PARAM_RUN_WM" in after
        else None
    ),
    "baselineBatchProgress": batch_delta,
    "scopePosition": scope_position,
}

show("2 second direct-read samples", samples)
show("After hand turn", after_rows)
show("Hand-turn diagnosis summary", summary)

print(
    "\nInterpretation: PARAM_RUN_WM is allowed to remain 0 while DISABLED; "
    "use exact Position and PARAM_ENCODER_WM for this hand-turn check.",
    flush=True,
)
print(
    "If you physically rotated the shaft but exact Position, Encoder WM and the existing "
    "Scope Position range all stayed unchanged while baseline batches kept arriving, "
    "the fault is below the GUI/Automation display path.",
    flush=True,
)
