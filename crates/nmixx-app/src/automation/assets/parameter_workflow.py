"""Template: change one explicitly chosen GUI parameter and inspect readback.

Set KEY and VALUE for your controller, save a local copy, and open that file in
Automation. This deliberately has no runnable default motor-control value.
"""
from nmixx import client
KEY = ""
VALUE = None
if not KEY or VALUE is None:
    raise RuntimeError("Set KEY and VALUE in your own local copy before running.")
print("Before:", client.cached([KEY]))
print("Canonical write/readback:", client.set(KEY, VALUE))
print("After:", client.cached([KEY]))
# A separate explicit client.call('config.save') saves to Flash when disabled.
