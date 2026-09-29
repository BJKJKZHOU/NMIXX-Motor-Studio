"""NMIXX Automation v1 client. Uses the parent's private stdio bridge, not USB.

No third-party Python packages are required. This client composes the current
application operations; it does not execute CLI commands or open a device.
It is not a sandbox: only run trusted scripts with your OS user privileges.
"""
from __future__ import annotations

import json
import os
import sys
import threading
from typing import Any


class ApiError(RuntimeError):
    def __init__(self, code: int, message: str, data: Any = None):
        super().__init__(message)
        self.code, self.data = code, data


class Client:
    def __init__(self) -> None:
        if os.environ.get("NMIXX_AUTOMATION_PROTOCOL") != "stdio-jsonrpc-v1":
            raise RuntimeError("Run this script from NMIXX Automation; do not open a device port.")
        self._writer = sys.stdout
        self._reader = sys.stdin
        self._lock = threading.Lock()
        self._sequence = 0
        # Keep print/tracebacks separate from protocol responses/requests.
        sys.stdout = sys.stderr

    def call(self, method: str, **params: Any) -> Any:
        if not isinstance(method, str) or not method:
            raise ValueError("method must be a non-empty string")
        with self._lock:
            self._sequence += 1
            request_id = self._sequence
            request = {"jsonrpc": "2.0", "id": request_id, "method": method, "params": params}
            self._writer.write(json.dumps(request, ensure_ascii=False, allow_nan=False) + "\n")
            self._writer.flush()
            line = self._reader.readline(4 * 1024 * 1024 + 1)
            if not line:
                raise RuntimeError("The Automation session ended or was cancelled.")
            if len(line) > 4 * 1024 * 1024 or not line.endswith("\n"):
                raise RuntimeError("Oversized or incomplete Application API response")
            response = json.loads(line)
            if not isinstance(response, dict) or response.get("jsonrpc") != "2.0" or response.get("id") != request_id:
                raise RuntimeError("Application API response ID/protocol mismatch")
            if "error" in response:
                error = response["error"]
                raise ApiError(error["code"], error["message"], error.get("data"))
            if "result" not in response:
                raise RuntimeError("Application API response has no result")
            return response["result"]

    def parameters(self) -> list[dict[str, Any]]:
        return self.call("parameter.list")

    def cached(self, keys: list[str]) -> list[dict[str, Any]]:
        """Snapshot before an explicit Read; Read itself also updates shared cache."""
        return self.call("parameter.cached", keys=keys)

    def read(self, keys: list[str]) -> list[dict[str, Any]]:
        """Actual device Read through the current ApplicationSession."""
        return self.call("parameter.read", keys=keys)

    def stream_progress(self) -> dict[str, Any]:
        return self.call("stream.progress")

    def scope_summary(self, window_seconds: float = 0.01) -> dict[str, Any]:
        return self.call("scope.summary", windowSeconds=window_seconds)


    def set(self, key: str, value: Any) -> dict[str, Any]:
        """The same write-and-readback operation as committing a GUI field."""
        metadata = next((m for m in self.parameters() if key in (m["symbol"], m["label"])), None)
        if metadata is None:
            raise ValueError(f"Unknown parameter: {key}")
        return self.call("parameter.set", key=key, value={"type": metadata["typeName"], "value": value})

    def wait(self, seconds: float) -> None:
        self.call("wait", seconds=seconds)

    def enable(self) -> dict[str, Any]:
        return self.call("motor.enable")

    def disable(self) -> dict[str, Any]:
        return self.call("motor.disable")

    def stop(self) -> dict[str, Any]:
        return self.call("motor.stop")

    def wait_stopped(self, timeout: float = 60) -> dict[str, Any]:
        return self.call("motor.wait_stopped", timeoutSeconds=timeout)

    def run(self) -> dict[str, Any]:
        """Execute committed Motion. Acceptance does not mean target arrival."""
        return self.call("motion.run")

    def identify(self, kind: str, timeout: float = 60, allow_enable: bool = False) -> dict[str, Any]:
        return self.call("identification.run", kind=kind, timeoutSeconds=timeout, allowEnable=allow_enable)

    def apply_identification(self, kind: str) -> dict[str, Any]:
        return self.call("identification.apply", kind=kind)


client = Client()
