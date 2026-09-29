"""Explicitly selected motion example: uses the CURRENT committed Motion target.

Select a suitable target and limits first. Enable the motor manually before Run.
This example stops the current Scope record and starts a new one deliberately.
No implicit Enable, Flash save, or hidden parameter changes are performed.
"""
from nmixx import client

state = client.read(["PARAM_MOTOR_STATE"])[0]
if state["error"] or state["value"] != {"type": "u8", "value": 1}:
    raise RuntimeError("Enable the motor first, after reviewing target and limits.")
print("Running the already committed Motion command for 1 second.")
client.call("scope.stop")
client.call("scope.configure", channels=[
    {"key": "PARAM_RUN_IQ", "rate": "normal"},
    {"key": "PARAM_RUN_WM", "rate": "normal"},
    {"key": "PARAM_RUN_POSITION", "rate": "normal"},
    {"key": "PARAM_ADC_VBUS", "rate": "normal"},
], historySeconds=10)
client.call("scope.live")
client.wait(0.2)
client.run()  # Acceptance, not a fabricated 'target reached' event.
client.wait(1.0)
client.stop()
client.wait_stopped(timeout=60)
client.wait(0.2)
client.call("scope.stop")
print(client.scope_summary(window_seconds=1.0))
print("Motion and recording stopped. Inspect the frozen Scope record.")
