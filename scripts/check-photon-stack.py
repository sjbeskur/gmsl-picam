#!/usr/bin/env python3
"""Run on an idle Pi: exercise UDP C2, TCP/Unix postcards, HTTP view, and reconnection."""
import argparse
import itertools
import json
import os
from pathlib import Path
import signal
import socket
import subprocess
import tempfile
import time
import urllib.error
import urllib.request


def free_port(kind=socket.SOCK_STREAM):
    with socket.socket(socket.AF_INET, kind) as sock:
        sock.bind(("127.0.0.1", 0))
        return sock.getsockname()[1]


def stop(process):
    if process is not None and process.poll() is None:
        process.send_signal(signal.SIGTERM)
        process.wait(timeout=5)
        assert process.returncode == 0, f"unclean exit: {process.returncode}"


def check(binary_dir, transport, directory, restart_baseline=False):
    udp_port, preview_port, control_port = free_port(socket.SOCK_DGRAM), free_port(), free_port()
    while preview_port == control_port:
        control_port = free_port()
    image_endpoint = (f"tcp://127.0.0.1:{free_port()}" if transport == "tcp"
                      else f"unix://{directory / 'images.sock'}")
    sequence = itertools.count(1)
    command_socket = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
    command_socket.connect(("127.0.0.1", udp_port))
    command_socket.settimeout(.4)
    daemon = viewer = None
    logs = []

    def launch(binary, *args):
        log = (directory / f"{binary}-{len(logs)}.log").open("wb")
        logs.append(log)
        return subprocess.Popen([str(binary_dir / binary), *args], stdout=log, stderr=subprocess.STDOUT)

    def command(kind, settings=None, expect_error=False):
        request_id = next(sequence)
        payload = {"version": 1, "request_id": request_id, "command": {"type": kind}}
        if settings is not None:
            payload["command"]["settings"] = settings
        payload = json.dumps(payload).encode()
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            assert daemon.poll() is None, "daemon exited"
            try:
                command_socket.send(payload)
                reply = json.loads(command_socket.recv(4097))
            except (socket.timeout, ConnectionRefusedError):
                time.sleep(.05)
                continue
            if reply["request_id"] != request_id:
                continue
            assert reply["version"] == 1, reply
            result = reply["result"]
            if expect_error:
                assert result["type"] == "error", result
                return result["data"]
            if result["type"] == "error":
                raise RuntimeError(result)
            return result.get("data")
        raise TimeoutError(kind)

    def ready():
        deadline = time.monotonic() + 12
        while time.monotonic() < deadline:
            status = command("status")
            assert status["camera"]["camera_state"] != "Failed", status
            if status["camera"]["frame_count"] >= 10 and not status["camera"]["frame_stale"]:
                return status
            time.sleep(.1)
        raise TimeoutError("camera did not become healthy")

    def jpeg():
        deadline = time.monotonic() + 8
        while time.monotonic() < deadline:
            assert viewer.poll() is None, "viewer exited"
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{preview_port}/frame.jpg", timeout=2) as response:
                    image = response.read()
                    assert image.startswith(b"\xff\xd8") and image.endswith(b"\xff\xd9")
                    return
            except (urllib.error.URLError, TimeoutError):
                time.sleep(.1)
        raise TimeoutError("preview did not become healthy")

    def launch_viewer():
        return launch("iris-view", "--bind", f"127.0.0.1:{preview_port}",
                      "--control-bind", f"127.0.0.1:{control_port}", "--listen", image_endpoint,
                      "--command-endpoint", f"udp://127.0.0.1:{udp_port}")

    try:
        viewer = launch_viewer()
        daemon = launch("irisd", "--idle", "--control-bind", f"127.0.0.1:{udp_port}",
                        "--transmit", image_endpoint, "--exposure-us", "59000", "--gain", "1.5")
        command("ping")
        assert command("status")["camera"]["camera_state"] == "Stopped"
        cli = subprocess.run([str(binary_dir / "iris"), "--endpoint", f"udp://127.0.0.1:{udp_port}",
                              "--config", str(directory / "client.toml"), "ping"],
                             capture_output=True, text=True, timeout=15)
        assert cli.returncode == 0 and cli.stdout.startswith("pong"), cli.stderr
        command("start")
        initial = ready()
        jpeg()
        invalid = command("settings", {"gain": 0}, expect_error=True)
        assert invalid["code"] == "invalid", invalid
        conflict = command("settings", {"width": 640}, expect_error=True)
        assert conflict["code"] == "conflict", conflict
        with urllib.request.urlopen(f"http://127.0.0.1:{preview_port}/status", timeout=2) as response:
            received = json.load(response)["latest_frame"]
            assert received["sensor_ts_ns"] > 0 and received["width"] == 1280
            assert abs(received["analogue_gain"] - 1.5) < .05
        with urllib.request.urlopen(f"http://127.0.0.1:{control_port}/ping", timeout=3) as response:
            assert response.status == 204
        # Measure delivered MJPEG, process CPU, and sensor publication rate together.
        def ticks(pid):
            fields = Path(f"/proc/{pid}/stat").read_text().rsplit(")", 1)[1].split()
            return int(fields[11]) + int(fields[12])
        first_ticks, view_ticks = ticks(daemon.pid), ticks(viewer.pid)
        first_count = command("status")["camera"]["frame_count"]
        start = time.monotonic()
        delivered = 0
        with urllib.request.urlopen(f"http://127.0.0.1:{preview_port}/stream.mjpg", timeout=3) as response:
            while time.monotonic() - start < 5:
                boundary = response.readline()
                if not boundary:
                    raise RuntimeError(f"preview stream ended: {command('status')}")
                assert boundary == b"--iris\r\n", boundary
                headers = {}
                while True:
                    line = response.readline()
                    if line == b"\r\n":
                        break
                    key, value = line.decode().split(":", 1)
                    headers[key.lower()] = value.strip()
                image = response.read(int(headers["content-length"]))
                assert image.startswith(b"\xff\xd8") and image.endswith(b"\xff\xd9")
                assert response.read(2) == b"\r\n"
                delivered += 1
        elapsed = time.monotonic() - start
        final = command("status")
        frequency = os.sysconf("SC_CLK_TCK")
        print(json.dumps({"transport": transport,
                          "daemon_cpu_one_core": (ticks(daemon.pid) - first_ticks) / frequency / elapsed * 100,
                          "viewer_cpu_one_core": (ticks(viewer.pid) - view_ticks) / frequency / elapsed * 100,
                          "published_fps": (final["camera"]["frame_count"] - first_count) / elapsed,
                          "mjpeg_fps": delivered / elapsed}), flush=True)
        # Restart just the viewer. Capture and its frame index must continue.
        stop(viewer)
        before = command("status")["camera"]["frame_count"]
        time.sleep(.5)
        assert command("status")["camera"]["frame_count"] > before
        viewer = launch_viewer()
        jpeg()
        assert command("status")["camera"]["frame_count"] > initial["camera"]["frame_count"]
        command("settings", {"gain": 2.0, "exposure_us": 45000, "jpeg_quality": 80})
        def wait_settings(exposure, gain):
            deadline = time.monotonic() + 8
            while time.monotonic() < deadline:
                frame = command("status")["camera"]["latest_frame"]
                if abs(frame["analogue_gain"] - gain) < .05 and abs(frame["exposure_us"] - exposure) < 100:
                    return
                time.sleep(.1)
            raise TimeoutError("live settings not applied")
        wait_settings(45000, 2.0)
        if restart_baseline:
            command("settings", {"gain": 1.5, "exposure_us": 59000, "jpeg_quality": 85})
            wait_settings(59000, 1.5)
        command("stop")
        assert command("status")["camera"]["camera_state"] == "Stopped"
        # Direct UDP stop propagates to HTTP freshness even without a browser command.
        time.sleep(2.2)
        try:
            urllib.request.urlopen(f"http://127.0.0.1:{preview_port}/frame.jpg", timeout=2)
            raise AssertionError("stopped camera served a stale image")
        except urllib.error.HTTPError as error:
            assert error.code == 503
        command("start")
        ready()
        jpeg()
        command("stop")
        print(f"PASS: {transport} UDP control, metadata/images, HTTP view, reconnect, live settings, stop/start (restart exposure: {59000 if restart_baseline else 45000} us)", flush=True)
    except Exception:
        for log in logs:
            log.flush()
        for path in directory.glob("*.log"):
            print(path, path.read_text(errors="replace"))
        raise
    finally:
        stop(daemon)
        stop(viewer)
        command_socket.close()
        for log in logs:
            log.close()


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("binary_dir", type=Path)
    parser.add_argument("--restart-baseline", action="store_true",
                        help="restore 59000 us / gain 1.5 before testing camera stop/start")
    args = parser.parse_args()
    for transport in ["tcp", "unix"]:
        with tempfile.TemporaryDirectory(prefix="iris-photon-") as temporary:
            check(args.binary_dir.resolve(), transport, Path(temporary), args.restart_baseline)


if __name__ == "__main__":
    main()
