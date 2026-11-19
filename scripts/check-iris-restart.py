#!/usr/bin/env python3
"""Daemon restart regression check (pass the irisd binary). Run on an idle Pi; uses only Python's standard library."""
import argparse
import json
import signal
import socket
import subprocess
import tempfile
import time
from pathlib import Path


def check_session(binary, exposure, gain, log):
    with socket.socket(socket.AF_INET, socket.SOCK_DGRAM) as probe:
        probe.bind(("127.0.0.1", 0))
        port = probe.getsockname()[1]
    control = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    control.connect(("127.0.0.1", port))
    control.settimeout(.2)
    with log.open("wb") as output:
        process = subprocess.Popen(
            [str(binary), "--no-transmit", "--exposure-us", str(exposure),
             "--control-bind", f"127.0.0.1:{port}", "--gain", str(gain), "--width", "1280", "--height", "720"],
            stdout=output, stderr=subprocess.STDOUT,
        )
        try:
            deadline = time.monotonic() + 15
            first_timestamp = None
            while time.monotonic() < deadline:
                if process.poll() is not None:
                    raise RuntimeError("irisd exited before capture")
                try:
                    request_id = time.monotonic_ns()
                    control.send(json.dumps({"version": 1, "request_id": request_id, "command": {"type": "status"}}).encode())
                    response = json.loads(control.recv(4097))
                    if response["request_id"] != request_id:
                        continue
                    assert response["result"]["type"] == "status", response
                    status = response["result"]["data"]["camera"]
                except (ConnectionRefusedError, TimeoutError):
                    time.sleep(0.1)
                    continue
                if status["camera_state"] == "Failed":
                    raise RuntimeError(status["last_error"])
                frame = status.get("latest_frame")
                if frame:
                    timestamp = frame["sensor_ts_ns"]
                    assert timestamp > 0 and not status["frame_stale"], status
                    if first_timestamp is None:
                        first_timestamp = timestamp
                    elif timestamp > first_timestamp and status["frame_count"] >= 10:
                        print(json.dumps(status))
                        break
                time.sleep(0.1)
            else:
                raise RuntimeError("No advancing capture within 15 seconds")
        finally:
            control.close()
            if process.poll() is None:
                process.send_signal(signal.SIGTERM)
                try:
                    process.wait(timeout=5)
                except subprocess.TimeoutExpired:
                    process.kill()
                    process.wait()
                    raise RuntimeError("SIGTERM shutdown exceeded 5 seconds")
        if process.returncode != 0:
            raise RuntimeError(f"irisd exited with {process.returncode}")


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary", type=Path)
    parser.add_argument("--exposure-us", type=int, default=59000)
    parser.add_argument("--gain", type=float, default=1.0)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix="iris-restart-") as directory:
        for session in range(2):
            log = Path(directory) / f"session-{session}.log"
            try:
                check_session(args.binary.resolve(), args.exposure_us, args.gain, log)
            except Exception:
                print(log.read_text(errors="replace"))
                raise
    print("PASS: two valid capture sessions and clean SIGTERM shutdowns")


if __name__ == "__main__":
    main()
