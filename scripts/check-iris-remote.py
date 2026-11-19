#!/usr/bin/env python3
"""Test a client on this machine against an idle irisd on another host over UDP."""
import argparse
import json
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--address", required=True)
    args = parser.parse_args()

    def call(*command):
        cmd = [str(args.client.resolve()), "--address", args.address]
        result = subprocess.run([*cmd, *command], capture_output=True, text=True, timeout=15)
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    status = call("status")
    assert status["camera"]["camera_state"] == "Stopped", "run on an idle daemon"
    try:
        call("set", "--exposure-us", "59000", "--gain", "1.5")
        call("start")
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            status = call("status")
            assert status["camera"]["camera_state"] != "Failed", status
            if status["camera"]["frame_count"] >= 10 and not status["camera"]["frame_stale"]:
                assert status["camera"]["latest_frame"]["sensor_ts_ns"] > 0
                print(json.dumps(status))
                break
            time.sleep(0.1)
        else:
            raise RuntimeError("remote capture did not become healthy")
    finally:
        status = call("stop")
        assert status["camera"]["camera_state"] == "Stopped"
    print("PASS: direct remote UDP status/settings/start/stop without credentials")


if __name__ == "__main__":
    main()
