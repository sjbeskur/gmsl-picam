#!/usr/bin/env python3
"""Run on an idle Pi to test irisd and iris together, including live controls."""
import argparse
import json
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request
from pathlib import Path


def free_port():
    with socket.socket() as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def check(binary_dir):
    preview, control = free_port(), free_port()
    while control == preview:
        control = free_port()
    address = f"127.0.0.1:{control}"
    def client(*command, success=True):
        result = subprocess.run([str(binary_dir / "iris"), "--address", address, *command],
                                capture_output=True, text=True, timeout=15)
        if success:
            assert result.returncode == 0, result.stderr
            return json.loads(result.stdout)
        assert result.returncode != 0, "invalid command was accepted"
        return result.stderr

    def wait_for(predicate):
        deadline = time.monotonic() + 12
        latest = None
        while time.monotonic() < deadline:
            assert daemon.poll() is None, "irisd exited"
            latest = client("status")
            assert latest["camera"]["camera_state"] != "Failed", latest
            if predicate(latest):
                return latest
            time.sleep(0.1)
        raise RuntimeError(f"timed out waiting for capture: {latest}")

    with tempfile.TemporaryDirectory(prefix="irisd-control-") as directory:
        log = Path(directory) / "daemon.log"
        with log.open("wb") as output:
            daemon = subprocess.Popen([str(binary_dir / "irisd"), "--idle", "--bind", f"127.0.0.1:{preview}",
                                       "--control-bind", address, "--exposure-us", "59000", "--gain", "1.5"],
                                      stdout=output, stderr=subprocess.STDOUT)
            try:
                for _ in range(50):
                    try:
                        with urllib.request.urlopen(f"http://{address}/status", timeout=1):
                            break
                    except (urllib.error.URLError, TimeoutError):
                        time.sleep(0.1)
                initial = client("status")
                assert initial["camera"]["camera_state"] == "Stopped"
                client("start")
                first = wait_for(lambda s: s["camera"]["frame_count"] >= 10 and not s["camera"]["frame_stale"])
                first_ts = first["camera"]["latest_frame"]["sensor_ts_ns"]
                assert first_ts > 0
                client("set", "--gain", "2.0", "--exposure-us", "45000", "--jpeg-quality", "80")
                live = wait_for(lambda s: (s["camera"]["latest_frame"] is not None
                    and abs((s["camera"]["latest_frame"]["analogue_gain"] or 0) - 2.0) < 0.05
                    and abs((s["camera"]["latest_frame"]["exposure_us"] or 0) - 45000) < 1000))
                assert live["camera"]["frame_count"] > first["camera"]["frame_count"], "capture restarted during settings update"
                assert live["camera"]["latest_frame"]["sensor_ts_ns"] > first_ts
                client("set", "--gain", "0", success=False)
                client("set", "--width", "640", success=False)
                assert client("status")["requested_settings"]["gain"] == 2.0
                with urllib.request.urlopen(f"http://127.0.0.1:{preview}/frame.jpg", timeout=2) as response:
                    jpeg = response.read()
                    assert jpeg.startswith(b"\xff\xd8") and jpeg.endswith(b"\xff\xd9")
                stopped = client("stop")
                assert stopped["camera"]["camera_state"] == "Stopped" and not stopped["capture_active"]
                try:
                    urllib.request.urlopen(f"http://127.0.0.1:{preview}/frame.jpg", timeout=2)
                    raise AssertionError("stopped camera served a frame")
                except urllib.error.HTTPError as error:
                    assert error.code == 503
                client("start")
                resumed = wait_for(lambda s: s["camera"]["frame_count"] >= 10 and not s["camera"]["frame_stale"])
                assert resumed["camera"]["latest_frame"]["sensor_ts_ns"] > first_ts
                assert daemon.poll() is None
                print(json.dumps(resumed))
                client("stop")
            except Exception:
                print(log.read_text(errors="replace"))
                raise
            finally:
                daemon.terminate()
                try:
                    daemon.wait(timeout=8)
                except subprocess.TimeoutExpired:
                    daemon.kill()
                    daemon.wait()
                    raise RuntimeError("irisd did not shut down cleanly")
            assert daemon.returncode == 0, log.read_text(errors="replace")
    print("PASS: daemon stayed running through live exposure/gain changes, camera stop/start, and rejected invalid settings")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary_directory", type=Path)
    check(parser.parse_args().binary_directory.resolve())
