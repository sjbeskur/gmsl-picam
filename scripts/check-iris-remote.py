#!/usr/bin/env python3
"""Test a client on this machine against an idle irisd on another host over TCP."""
import argparse
import json
import subprocess
import time
from pathlib import Path


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("client", type=Path)
    parser.add_argument("--address", required=True)
    parser.add_argument("--token-file", required=True, type=Path)
    args = parser.parse_args()

    def call(*command, authenticated=True):
        cmd = [str(args.client.resolve()), "--address", args.address]
        if authenticated:
            cmd += ["--token-file", str(args.token_file)]
        result = subprocess.run([*cmd, *command], capture_output=True, text=True, timeout=15)
        if not authenticated:
            assert result.returncode != 0 and "401" in result.stderr, result.stderr
            return None
        assert result.returncode == 0, result.stderr
        return json.loads(result.stdout)

    call("status", authenticated=False)
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
    print("PASS: direct remote TCP status/settings/start/stop; missing credentials rejected")


if __name__ == "__main__":
    main()
