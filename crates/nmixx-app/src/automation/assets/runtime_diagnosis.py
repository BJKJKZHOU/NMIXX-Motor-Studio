"""Read-only diagnosis. Does not Enable, Run, Stop, Save or reconfigure Plot."""
import json
import time

from nmixx import client


def show(label, value):
    print("
=== " + label + " ===", flush=True)
    print(json.dumps(value, ensure_ascii=False, indent=2, allow_nan=False), flush=True)


registry = client.parameters()
available = {item["symbol"] for item in registry}
requested = [
    "PARAM_MOTOR_STATE", "PARAM_MOTOR_MODE", "PARAM_ADC_VBUS",
    "PARAM_RUN_IQ", "PARAM_RUN_WM", "PARAM_RUN_POSITION",
    "PARAM_ENCODER_PROTOCOL", "PARAM_ENCODER_SPI_TYPE", "PARAM_ENCODER_READY",
    "PARAM_ENCODER_VALID", "PARAM_ENCODER_FAULT", "PARAM_ENCODER_FAULT_REASON",
    "PARAM_ENCODER_WM", "PARAM_CAL_VALID",
]
keys = [key for key in requested if key in available]
print("READ-ONLY: using the connected ApplicationSession, no separate transport.", flush=True)
show("Not exposed by the loaded schema", [key for key in requested if key not in available])

# A successful direct Read may update the cache; keep the pre-Read snapshot.
show("Shared cache BEFORE device Read", client.cached(keys))
show("Direct device Read (one sequential read group, not one atomic sample)", client.read(keys))
show("Shared cache AFTER device Read", client.cached(keys))

first = client.stream_progress()
show("Receive progress, start", first)
print("Observing 1 second of reception. Equal numeric values do not imply a stalled stream.", flush=True)
time.sleep(1.0)
last = client.stream_progress()
show("Receive progress, end", last)
first_by_id = {item["id"]: item for item in first["channels"]}
progress = []
for item in last["channels"]:
    previous = first_by_id.get(item["id"])
    progress.append({
        "id": item["id"], "symbol": item["symbol"],
        "receivedBatchDelta": None if previous is None else item["receivedBatches"] - previous["receivedBatches"],
        "ageMs": item["ageMs"],
    })
show("Batch arrival progress (NOT firmware sampling-rate measurement)", progress)
show("Scope record summary (does not change channels or Run/Stop)", client.scope_summary())
print("
Diagnosis completed. Position parameter is turn+rad; Plot position is f32 turns.", flush=True)
print("Different sample times and representations must not be compared as exact equality.", flush=True)
