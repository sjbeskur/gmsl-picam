#!/usr/bin/env python3
"""Sample irisd CPU, delivered FPS and estimated sensor-to-publication age.

Run on the Pi: python3 measure-iris-performance.py --pid <irisd-pid>
The age estimate uses the sensor's CLOCK_BOOTTIME timestamp and excludes the
age of the already-published JPEG. It is not browser end-to-end latency.
"""
import argparse
import json
import os
import statistics
import socket
from urllib.parse import urlparse
import time
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="udp://127.0.0.1:8081")
    parser.add_argument("--seconds", type=float, default=10)
    parser.add_argument("--pid", type=int, required=True)
    args = parser.parse_args()
    if args.seconds <= 0:
        parser.error("--seconds must be positive")

    def ticks():
        with open(f"/proc/{args.pid}/stat", encoding="utf-8") as stream:
            fields = stream.read().rsplit(")", 1)[1].split()
        return int(fields[11]) + int(fields[12])

    def sample():
        endpoint = urlparse(args.url)
        if endpoint.scheme == "udp":
            request_id = time.monotonic_ns()
            payload = json.dumps({"version": 1, "request_id": request_id, "command": {"type": "status"}}).encode()
            peer = socket.getaddrinfo(endpoint.hostname, endpoint.port, type=socket.SOCK_DGRAM)[0]
            with socket.socket(peer[0], socket.SOCK_DGRAM) as control:
                control.settimeout(3)
                control.connect(peer[4])
                control.send(payload)
                response = json.loads(control.recv(4097))
            assert response["version"] == 1 and response["request_id"] == request_id, response
            assert response["result"]["type"] == "status", response
            status = response["result"]["data"]
        else:
            with urllib.request.urlopen(args.url.rstrip("/") + "/status", timeout=3) as response:
                status = json.load(response)
        camera = status["camera"]
        if camera["camera_state"] != "Running" or camera["frame_stale"]:
            raise RuntimeError(f"capture is not healthy: {camera}")
        return status

    first = sample()
    start = time.monotonic()
    first_ticks = ticks()
    ages = []
    seen = set()
    while True:
        time.sleep(min(0.1, max(0, args.seconds - (time.monotonic() - start))))
        last = sample()
        camera = last["camera"]
        frame = camera["latest_frame"]
        if frame["frame_index"] not in seen:
            seen.add(frame["frame_index"])
            ages.append(time.clock_gettime(time.CLOCK_BOOTTIME) * 1000
                        - frame["sensor_ts_ns"] / 1_000_000 - camera["frame_age_ms"])
        if time.monotonic() - start >= args.seconds:
            break
    elapsed = time.monotonic() - start
    cpu = (ticks() - first_ticks) / os.sysconf("SC_CLK_TCK") / elapsed * 100
    ages.sort()
    print(json.dumps({
        "seconds": round(elapsed, 3), "cpu_percent_one_core": round(cpu, 2),
        "published_fps": round((last["camera"]["frame_count"] - first["camera"]["frame_count"]) / elapsed, 2),
        "publication_age_ms_median": round(statistics.median(ages), 2),
        "publication_age_ms_p95": round(ages[min(len(ages) - 1, int(len(ages) * .95))], 2),
        "requested_settings": last["requested_settings"], "latest_frame": frame,
    }, indent=2))


if __name__ == "__main__":
    main()
