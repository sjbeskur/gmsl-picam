# Iris session handoff

Date: 2026-11-16 (America/Denver)
Session tag: `iris-reliability-2026-11-16`

## 2026-11-18: support contacted and exact product specifications located

The user emailed Arducam support and expects the response may take a couple of
days. They supplied the exact Raspberry Pi 12MP extension-kit product page:
https://www.arducam.com/arducam-gmsl2-12mp-camera-extension-kit-for-raspberry-pi.html

Direct retrieval returned HTTP 403, but the manufacturer's indexed product text
lists the adapter at 5 V / 1 A, camera at 3.3 V / 500 mA, consumption 6.6 W,
and operating temperature -20 to +85 C. These values do not specify idle
camera-head case temperature, thermal cutoff, the power split across the kit,
or the RXA indicator's electrical meaning. Do not equate the listed operating
range with an acceptable touch temperature, an internal protection threshold,
or proof that the reported behavior is normal. The generic kit introduction's
adapter-only power figures are distinct from this product listing.

The official Raspberry Pi quick-start guide also says RX adapter firmware is
SKU-specific and warns against mixing camera modules/receiver boards from
different SKUs. This supports confirming that the components are the matched
B0550 kit before interpreting the fault. No I2C commands, DIP changes, added
external power, camera tests, or deployment were performed. The hardware remains
unpowered at the user's last confirmation; offline software work can continue
with synthetic frames while awaiting clarification.
Reference:
https://docs.arducam.com/GMSL-Camera-Solution/GMSL-Camera-for-Raspberry-Pi/Quick-Start-Guide/

## 2026-11-18: heated head and RXA LED location clarified

The user confirmed two distinct locations: the remote camera head at the far
end of the coax gets hot, and the green indicator near RXA on the Pi-side
UC-C44 Rev.E receiver goes out. The Pi remains unplugged at the last explicit
power-state confirmation. No hardware tests were performed for this clarification.

This directs investigation to the remote head / coax / receiver channel-A path.
The RXA LED's exact function is still unverified; it must not be identified as
PoC power-good or GMSL lock solely from its port label. If it indicates channel
power, a loss could indicate an output-power/control problem; if it indicates
lock, loss could occur while the camera remains powered. A board schematic or
Arducam's revision-specific confirmation is needed. The reported relation to
warming has no measured temperature, timing correlation, rail voltage, or fault
register confirmation, and does not prove thermal protection or a damaged sensor.

A concise vendor diagnostic question is: on UC-C44 Rev.E, what drives the RXA
indicator, is PoC output protected by thermal/current cutoff, what idle case
temperature is expected for the matching IMX477 head, and which supported status
readouts distinguish power loss from link loss? Include official 27W Pi PSU
5.1 V / 5.0 A, Pi-fed adapter with no separate supply, connected port RXA,
frontend timeout/cancelled requests, and heating observed with irisd stopped.
No support message has been sent. Keep capture/deployment paused while the
power/thermal concern is unresolved; the startup-controls candidate is untested.

## 2026-11-18: board photo identifies UC-C44 Rev.E receiver

The user supplied a photo of the Pi-mounted ArduCam UC-C44 Rev.E board,
labelled "Dual GMSL2 to CSI-2 Deserializer Board", and identified RXA as the
connected coax port. This is the Pi-side receiver, not a photograph of the
remote sensor head. Visible silkscreen includes PWR/RXA/RXB indicator labels
and separate LOCK/ERRB labels. The exact wiring/meaning of the RXA indicator
has not been established from a schematic or revision-specific manual. Do not
assume RXA means link lock, power-good, or thermal status solely from the label.

Asked the user to distinguish the heated part (remote camera head, Pi-side
UC-C44, or both) and the location of the LED that goes out (RXA area on the
receiver versus the remote head). This clarification is pending. Earlier
wording referred to a camera-head indicator; the photo makes identifying its
actual location important. User last explicitly confirmed the Pi is unplugged;
no power-up, SSH, driver probing, or capture was attempted for this photo review.
Do not apply external-power or DIP-switch instructions from another GMSL board
revision/platform without verifying this board's documentation.

## 2026-11-18: official Pi PSU rating confirmed

The user additionally observed that the green camera-head indicator goes out
as the module warms (a perceived temperature threshold, not a measured one).
They explicitly confirmed that the Pi is still unplugged now. No hardware was
powered or accessed for this follow-up. The LED's exact function on this board
is unverified; do not infer thermal shutdown or loss of supply from its color or
state alone. Power/protection, GMSL link state, and driver-controlled indication
remain possible explanations. Asked for the unpowered board's LED label and
model/revision. These are pending; do not power up merely to reproduce the LED
change. Arducam's general camera pinout has a separate LED-control signal, but
that does not identify this specific GMSL board indicator.

The user identified the Pi supply as the Raspberry Pi 27W USB-C model, labelled
5.1 V / 5.0 A. This matches the official supply intended for Raspberry Pi 5;
an obviously undersized or incorrect-rated adapter is not supported by this
information. The rating does not establish actual voltage/current at the
camera, rule out a defective supply/connection/PoC regulator, or diagnose the
reported idle heating. A 5 A capacity is not a current forced into the camera.

Hardware tests remain paused. No new capture or power-cycle test was run after
the report of rapid/uncomfortable heating. The next useful evidence is measured
camera-head temperature and Arducam confirmation of expected idle behavior for
the actual kit, plus inspection of the unpowered connections. No hardware
thermal limit has been established from the adapter-only specifications.
Official PSU reference:
https://www.raspberrypi.com/products/27w-power-supply/

## 2026-11-18: rapid/uncomfortable idle heating; power-off advised

On another power-up, the user explicitly reported the camera head was getting
hot quickly or uncomfortable to touch with capture stopped. No capture or
restart command was sent by the assistant this boot. The attempted read-only
SSH connection timed out, so current service state and supply telemetry were
not independently verified on that boot.

Advised shutting down and disconnecting the Pi USB-C supply, or disconnecting
power promptly if shutdown is unavailable while heating continues. Avoid
handling the coax while powered and repeated temperature checks by touch.
No further power-up/capture/restart tests should be run while this concern is
unresolved. Hardware tests and deployment remain paused. This report does not
establish a measured overtemperature or identify whether powered-board
dissipation, driver state, regulator/power trouble, or another fault is involved.

Asked for the exact Pi power supply model and voltage/current label after power
off. That answer and confirmation of power-off are pending. The Pi-fed adapter /
PoC / camera arrangement needs checking against the actual Arducam kit's power
requirements and expected idle thermal behavior. The startup-controls candidate
remains built but hardware-untested and uninstalled.

## 2026-11-18: renewed warmth before capture; hardware tests stopped

The user rebooted with the camera attached and reported the Pi was up. A
read-only `cam --list` detected one external IMX477. The installed UDP endpoint
refused connections because `irisd.service` had been explicitly stopped:
its process exited successfully about a minute before the check. `iris-view`
remained active, but no irisd process or UDP command listener was running.
No capture command was issued and no candidate daemon was launched this boot.

Before any capture test, the user reported the module seemed overheated again.
All camera testing was stopped. Advised a clean Pi shutdown followed by removing
its power supply to fully cut Pi-fed PoC power, and no powered coax changes.
The report is still subjective: no measured module temperature establishes
overheating. Warmth with irisd stopped makes active Iris capture an insufficient
explanation; boot-time driver state, PoC/regulator behavior, and normal powered
board dissipation remain unresolved possibilities.

The candidate bundle was being copied to `/tmp/iris-startup` but no installed
binary/service was replaced. The startup-controls change remains hardware-untested.
Do not launch capture or resume restart tests until the physical temperature/power
concern has been addressed and the user is ready.

## 2026-11-18: Pi-fed power-over-coax confirmed; capture remains stopped

The user confirmed the disconnected connection is the GMSL/coax at the camera
head. The camera has a single cable, and the Pi-side adapter has no separate
power lead: it is powered only through its Pi connections. Arducam documents
power-over-coax for this kit. This identifies the likely power path but does not
establish overheating or correct camera-side rail voltage. No cable reconnection
or camera capture has been attempted during this check.

Read-only SSH became available. The daemon reported Stopped, zero frames, and
capture_active=false. Pi thermal_zone0 was 43000 (43 C), vcgencmd get_throttled
reported 0x0, kernel release was 6.8.0-1068-raspi, and imx477 module vermagic
matched that release. These are Pi measurements with the camera disconnected,
not camera-head temperature or a test of power stability under camera load.
Kernel boot messages show imx477 chip-ID read/probe failed with error -5, which
is compatible with the camera being disconnected and does not itself establish
a bad driver. Reconnect only with power off, and have the camera connected
before the next cold boot so the sensor can probe normally.

Hardware tests remain paused pending user readiness. The startup-controls
candidate in out/iris-startup remains uninstalled and hardware-untested.
Arducam reference:
https://docs.arducam.com/GMSL-Camera-Solution/GMSL-Camera-for-Raspberry-Pi/Introduction/

## 2026-11-18: camera disconnected; power details pending

After cooling, the user said the Pi was booting again, but SSH to
192.168.50.150 timed out. No candidate binary was deployed or camera test run.
The user then reported that the camera module warmed immediately and that they
rebooted and unplugged the camera. Keep hardware testing paused and do not
attempt capture while it is disconnected. Immediate warmth was reported by
touch; no module temperature, supply voltage, current, or fault measurement has
been obtained, so neither normal dissipation nor overheating/electrical fault
has been established.

Asked which connection was unplugged (coax, CSI ribbon, or separate power) and
how the camera/GMSL hardware is powered, including external supply voltage and
current rating. These answers are pending. Before reconnecting, the Pi and any
external camera/GMSL supply should be powered off. Verify the actual kit and
power arrangement before restarting capture. Avoid blind I2C writes or repeated
start/reboot cycles while the power arrangement remains unknown.

## 2026-11-18: installed live-view failure and camera cooling

The user installed the Photon bundle. Both `irisd.service` (PID 1841 at the
check) and `iris-view.service` (PID 1842) were active. UDP 8081, HTTP 8080/8081,
and postcard TCP 9001 listeners were present. The daemon failed before publishing
any image with a libcamera frontend timeout / cancelled request; the viewer
therefore correctly returned HTTP 503. Configuration remained 59000 us / gain 1.5
and `IRIS_TRANSMIT=tcp://127.0.0.1:9001`. A clean control stop/start still failed.
Running the same installed binary headlessly as the login user also failed,
so this is not explained solely by the systemd account or the HTTP viewer.

Source inspection found that requested controls were supplied only to the first
queued request while `Camera::start` received no controls. The vendored Pi
pipeline starts its IPA and applies initial sensor controls in `start()` before
streaming. `camera.rs` now supplies the first request's populated control list
to `cam.start(Some(startup_controls))`, while retaining the request controls for
normal operation. The candidate ARM bundle built in `out/iris-startup`; existing
workspace tests passed. **This candidate has not been installed or tested on the
camera, and is not a proven fix for the timeout.** The installed services still
use the preceding Photon build. Do not claim live view was restored.

The user then reported the camera head near the sensor was warm but touchable,
and is letting it cool. No camera temperature or supply voltage was measured;
overheating has not been established. Hardware capture/restart tests are paused
until the user is ready. Capture was stopped at the last successful control
check, and temporary test processes had exited. A subsequent SSH attempt timed
out, so its current powered state was not verified. Software stop alone must not
be treated as confirmation that camera/GMSL power has been removed.

Next: when the user is ready, verify the physical setup/power state, test the
startup-controls candidate in isolation without replacing the installed service,
and check actual advancing frames and JPEG delivery before deployment. Avoid
repeated blind camera starts while temperature/power concerns remain unresolved.
Relevant workstation logs: `/tmp/iris-service-vs-login.log`,
`/tmp/iris-startup-build.log`, and `/tmp/iris-startup-tests.log`.

## 2026-11-18: post-reboot validation at the established baseline

The user rebooted the Pi. `/tmp/iris-photon` had been cleared, so the final
bundle and test scripts were staged again. The installed HTTP daemon remained
active with capture stopped; no system service was replaced.

The default full-stack check recovered initial capture: UDP control, JPEG/MJPEG,
metadata delivery, viewer restart/reconnection, and live 45000 us / gain 2.0
settings all worked. It then failed while restarting camera capture with those
45000 us settings (`capture request was not completed: Cancelled`, libcamera
frontend timeout). This run is not a full pass. Log on workstation:
`/tmp/iris-photon-post-reboot.log`.

Two clean headless daemon sessions at 59000 us / gain 1.5 subsequently passed,
including advancing sensor timestamps and clean SIGTERM shutdowns. Log:
`/tmp/iris-photon-baseline-restart.log`.

Added an explicit `--restart-baseline` option to `check-photon-stack.py`. It
still verifies live 45000 us / gain 2.0 controls, then restores 59000 us / gain 1.5
and quality 85 before testing camera stop/start. The default test retains its
45000 us restart and still exposes that unresolved failure. Both TCP and Unix
full-stack checks passed with the baseline option: UDP commands, rejected invalid
and conflicting settings, JPEG/metadata delivery, MJPEG, viewer restart while
capture continued, live updates, stopped-frame 503, camera restart, and clean
process cleanup. Log: `/tmp/iris-photon-baseline-stack.log`.

Short five-second samples measured daemon CPU 31.86% (TCP) / 32.18% (Unix) of one
core, viewer CPU 0.20% / 0.40%, publication about 16.5–16.6 FPS, and MJPEG about
16.7–16.8 FPS including the initially available frame. Sensor frame period is
60.985 ms, approximately 16.4 FPS. Use the 59000 us / gain 1.5 baseline for
installation; do not claim arbitrary exposure settings restart reliably.

The staged bundle is again at `/tmp/iris-photon`. After user installation, the
HTTP preview URL remains unchanged. Installation requires interactive sudo:

```bash
sudo bash /tmp/iris-photon/install-irisd.sh
iris --endpoint udp://127.0.0.1:8081 start
```

This supersedes the earlier statement that no final hardware run succeeded:
baseline full-stack and headless restart checks now pass, while the default
45000 us camera restart regression remains open.

## 2026-11-18: Photon UDP control and separate iris-view implemented

The requested transport refactor is implemented. `photon-protocol` now owns
settings/status/frame types, versioned command/reply structs, and image postcard
framing. `iris-api` re-exports these types for compatibility. Commands are bounded
JSON UDP datagrams with request IDs, matching replies, 400 ms client retries,
and a bounded 30-second/256-entry daemon deduplication cache. A 12-second client
timeout leaves the command outcome unknown; this is not persistent exactly-once
execution or an ordering guarantee. Commands remain unauthenticated for home dev.

`irisd` contains the serialized camera controller and optional image output;
it no longer serves HTTP. The `Transmitter` trait has TCP and Unix implementations.
Its output worker observes the latest frame, shares the JPEG allocation, bounds
connect/write waits, coalesces slow-consumer updates, and reconnects without
blocking capture. `--no-transmit` / `IRIS_NO_TRANSMIT=true` supports headless use.
Postcards use a 16-byte big-endian header (`PHTN`, version, JPEG encoding,
metadata length, image length), then JSON metadata and raw JPEG bytes. Limits
are 16 KiB metadata and 32 MiB JPEG; lengths are checked before payload allocation.

`iris-view` receives postcards and serves the existing JPEG/MJPEG/status routes
on TCP 8080. Its optional browser control routes on TCP 8081 forward commands to
UDP 8081. Defaults use a loopback TCP image receiver on 9001. Native `iris` defaults
to `udp://127.0.0.1:8081`; existing HTTP profiles still work through the viewer.
`iris-client` supports native UDP-only builds and browser HTTP builds. Daemon and
UDP-only client dependency trees were checked to exclude Axum/Hyper/Reqwest/HTTP.
The production Docker recipe builds `irisd` separately with only `iris-service`,
then builds the client/viewer, preserving the daemon's feature isolation.

Packaging now stages three binaries, both systemd units, and both environment
files. `just deploy-iris` copies the full bundle. The installer preserves existing
camera defaults and adds an output endpoint once; repeated staged installations
were checked. `iris-view.service` creates `/run/iris-view` for a shared Unix socket;
the services' private `/tmp` namespaces are separate. For custom ports/addresses,
configure the viewer's UDP command endpoint to match the daemon. See the
[wire protocol, deployment, and manual commands](./irisd-service.md).

Validation completed: 51 unit/integration tests plus one doc test passed;
UDP-only client tests (4 plus its doc test) passed; browser wasm/config compilation
and Clippy passed; ARM release binaries built. Tests cover duplicate/lost replies,
invalid commands/settings, oversized/truncated/fragmented frames, TCP/Unix image
and metadata delivery, slow consumers, viewer control forwarding, and shutdown.

An initial full Pi run passed both TCP and Unix image transports, direct UDP CLI
ping, HTTP preview, viewer restart/reconnection while capture continued, live
settings, camera stop/start, and clean process exits. At the same 1280×720,
59000 us, gain 1.5, quality 85 baseline, daemon CPU was about 32.4% of one core and
viewer CPU about 0.4%, with about 16.4 published FPS (short 5-second windows
reported 16.5–16.6 FPS and 16.7–16.8 MJPEG FPS including the initial frame).

After the Pi disconnected/reconnected (installed daemon PID changed from 742 to
739), the final bundle rerun received frames initially but then hit the libcamera
frontend dequeue timeout and cancelled requests. The installed, unchanged HTTP
daemon subsequently also failed with `capture request was not completed:
Cancelled` and zero frames. This prevents claiming a successful final hardware
rerun: the camera/link needs recovery first. All experimental processes were
cleanly stopped and the installed daemon was restored to capture stopped. Its
system service and libraries were not replaced. Initial passing results are in
`/tmp/iris-photon-hardware.log` on the workstation; the failed rerun is in
`/tmp/iris-photon-hardware-final.log`. Do not silently treat this as a passing run.

The final install bundle is staged locally in `out/iris-photon` and on the Pi in
`/tmp/iris-photon`; the three binary SHA-256 hashes match. Installation still
requires the user's interactive sudo password. After camera/link recovery, run
on the Pi:

```bash
sudo bash /tmp/iris-photon/install-irisd.sh
iris --endpoint udp://127.0.0.1:8081 start
```

The browser URL remains `http://192.168.50.150:8080/stream.mjpg`. On the
workstation, `just install-iris` installs the new UDP-capable CLI, then use
`iris config front --endpoint udp://192.168.50.150:8081` for direct daemon control.
`python3 scripts/check-photon-stack.py /tmp/iris-photon` is the full hardware
regression check to rerun once the camera is healthy. The headless restart check
was updated for UDP but did not run in the final sequence because the preceding
stack check failed. The local unit/integration tests remain passing.

## 2026-11-18: NV12 and performance implementation documentation

Expanded [Journey of a photon](<./journey of a photon.md#what-nv12-looks-like>)
with the NV12 Y/UV layout, chroma sharing, payload sizes, the reusable-buffer
copy and conversion code, and the measured CPU/FPS/publication-age results.
It explains which buffers are reused, the allocations/copies that remain, and
why synthetic heap-backed benchmarks miss the live camera mapping penalty.
Added links from the documentation index and crate benchmark guide. This was
a documentation-only follow-up; it did not change the Pi's service or capture.

## 2026-11-17: CPU and preview performance fix

The capture worker now keeps one `Nv12JpegEncoder`, reusable packed NV12/RGB/JPEG
scratch buffers, fixed-point BT.601/709 conversion, and a statically linked
SIMD-enabled libjpeg-turbo compressor. It preserves full/limited range and the
previous encoder's actual 4:4:4 component sampling. The `image` crate is now only
a test/benchmark dependency. Native JPEG builds need CMake, a C compiler, and
NASM on x86; the ARM Docker build includes them and explicitly keeps executable
Python dependencies on amd64 to avoid an apt multiarch installation failure.

The important live-only bottleneck was byte-at-a-time access to mapped camera
memory: even after the first encoder change, conversion took around 52–58 ms.
A sequential copy into reusable cacheable NV12 memory takes about 1 ms and brings
conversion down to roughly 5–6 ms; JPEG compression takes about 8 ms. This extra
copy is intentional and much faster than directly walking the DMA mapping.
Enable `RUST_LOG=gmsl_picam_rs::jpeg=debug` for copy/conversion/compression timings.

Pi-local measurements at the same 1280×720, 59000 us exposure, gain 1.5, requested
30 FPS, JPEG quality 85 (actual frame period 60.985 ms):

| Measurement | Installed baseline | Optimized test build |
| --- | ---: | ---: |
| Process CPU, percent of one core | 101.49% | 32.53% |
| Published frames/sec | 6.20 | 16.40 |
| Estimated publication age, median | 1262.47 ms | 40.62 ms |
| Estimated publication age, p95 | 1289.82 ms | 42.95 ms |

Baseline sampling lasted 10 seconds; optimized sampling lasted 15 seconds with
stage debug logging enabled. Publication age uses CLOCK_BOOTTIME minus sensor
timestamp minus reported frame age; it is an estimate, not measured browser or
optical end-to-end latency. `scripts/measure-iris-performance.py` reproduces this
sampling. A 50-iteration standalone synthetic benchmark measured 48.126 ms for
the previous pipeline (18.126 ms conversion, 30.000 ms JPEG) versus 17.187 ms for
the initial optimized encoder. Synthetic heap-backed images do not reproduce
the mapped-memory penalty, so the live measurements are the deployment evidence.

MJPEG now waits for watch notifications instead of polling at 100 ms. Updates
coalesce for slow viewers; failed/stopped/stalled capture still ends streams.
A real Pi-local stream delivered 84 valid JPEGs in 5.04 seconds (16.68 FPS,
including the initially available frame). No resolution, quality, or exposure
reduction was used for the performance comparison.

Validation: 45 unit/integration tests and one doc test passed, Clippy passed
(existing optional-feature unused tracing-subscriber warning remains), the
iris-client wasm/config check passed, ARM release binaries built with NEON
symbols present, and the Pi passed live exposure/gain/quality updates, stop/start,
invalid-setting rejection, valid JPEG delivery, and two clean SIGTERM/restart
sessions. All temporary test daemons were stopped. The original installed daemon
is active with capture stopped, matching the state at the start of this work.

The tested install bundle is staged on the Pi at `/tmp/iris-performance` and
locally in `out/iris-performance`. Installing it needs an interactive sudo
password on the Pi, which SSH automation cannot supply. To activate there:

```bash
sudo bash /tmp/iris-performance/install-irisd.sh
iris start
```

Preview remains `http://192.168.50.150:8080/stream.mjpg` after installation. The
installed service itself has not yet been replaced. This optimization does not
address the separate sensor/CFE/GMSL failure that previously needed power cycling.

## 2026-11-17: preview latency measurements

After recovery, read-only sampling found about 1.30–1.35 seconds between each
published JPEG's SensorTimestamp and publication time, estimated by subtracting
reported frame age from CLOCK_BOOTTIME minus sensor timestamp. This includes
exposure/readout and queued processing; it is not a direct browser latency test.
Pi-local snapshot HTTP delivery was approximately 0.6–0.7 ms after warmup. The
capture/encoding thread used about 99% of one CPU, while CameraManager/IPA threads
were mostly idle; CPU frequency was 2.4 GHz and vcgencmd reported throttled=0x0.
Published throughput was approximately 5–7 frames/sec despite actual sensor
FrameDuration around 60.985 ms. At that point the preview also polled at 100 ms.

The CPU-bound capture/encoding path and buffering need profiling/optimization;
this evidence does not isolate conversion versus JPEG compression, nor measure
browser/network buffering. No settings or running service were changed for the
latency investigation. Capture remains running at 59000 us and gain 1.5. Treat the
reset/power-cycle failure separately from this steady-state processing delay.

## 2026-11-17: capture recovered after power cycle

Following the missing-video diagnosis below, the user power-cycled the hardware.
The Pi came back with the installed token-free systemd daemon active and capture
Stopped (`IRIS_IDLE=true` preserved). `iris camera front start` then recovered
healthy capture: 102 frames at the status check, valid sensor timestamps, measured
58998 us exposure, gain 1.4992679, frame duration 60985 us, no error and fresh frames.
The snapshot endpoint returned HTTP 200 with a valid 1280x720 JPEG (53374 bytes).
Capture was left running at requested 59000 us exposure and gain 1.5. The sensor/
link/driver cause of the earlier latched timeout remains undetermined.

## 2026-11-17: missing-video diagnosis

The updated systemd daemon is installed, active, and reachable without credentials.
A capture start produced zero frames and a PiSP/CFE frontend timeout; requests
were cancelled and the image endpoint correctly returned 503. One clean retry at
45000 us also failed. Capture was stopped and requested settings restored to
59000 us exposure and gain 1.5.

A bounded standalone `/usr/local/bin/cam -c 1 --capture=5`, run as the login user
outside the service, also timed out before any frame. It selected 800x600 XRGB8888
output with a 2028x1520 raw sensor mode, so the symptom occurs outside Iris and
outside its 1280x720 NV12 configuration. The sensor still enumerates. Current
service capture is Stopped; no test capture was left running. Next action is a
camera/GMSL power-cycle or reconnect, which recovered the same symptom in earlier
sessions, followed by checking advancing timestamps and a real JPEG response.
The underlying sensor/link/driver cause has not been identified.

## 2026-11-17: token-free home development

The user requested removal of token authentication for this home LAN. The latest
code accepts direct TCP control without credentials. The installer stops creating
keys; the daemon ignores the old token-file environment setting. Legacy client
`token_file` TOML fields are ignored (even if the referenced file is missing) and
dropped on the next config save. Token CLI flags and bearer client methods were
removed. The non-secret request header remains automatic for control mutations.

Use `iris config front --endpoint http://192.168.50.150:8081`, `iris ping front`,
and `iris camera front start|stop|status|set`. Capture URLs remain
`http://192.168.50.150:8080/stream.mjpg` and `/frame.jpg`.

Token-free validation passed 41 native tests and a client documentation example,
plus native/ARM and wasm32 builds. The workstation utility is updated. The Pi's
`/tmp/iris-install` bundle is refreshed; a separate idle daemon on port 18081
accepted remote ping/status with no credentials and stayed Stopped with zero
frames. That probe was stopped. Activating the actual installed service is still
pending `sudo bash /tmp/iris-install/install-irisd.sh` on the Pi: noninteractive
sudo was unavailable. Existing `IRIS_IDLE=true` was checked and will be preserved.

Earlier notes describing credential setup below record the previous design and
are superseded by this section.

## Daemon/client follow-up

The working tree now splits the previous service executable into `irisd` and an
`iris` control client. See [the current service guide](irisd-service.md) for build,
installation, API, and validation commands. The tagged session described below
predates this split: its `/tmp/iris` command is the old daemon, not the new client.

The new install bundle is exported under `out/iris-daemon`. `just deploy-iris`
copies both binaries and the installer/unit/defaults to `/tmp/iris-install`.
Persistent defaults live in `/etc/iris/irisd.env`. Runtime gain/exposure settings
can change without a capture restart. Camera start/stop is separate from systemd
service start/stop. The installed control API uses authenticated HTTP/JSON over TCP on port 8081;
preview stays on port 8080. The installer generates `/etc/iris/control.token`;
remote clients use `--address <pi>:8081 --token-file <private-file>`. Manual daemon
startup defaults to loopback and requires a token file for non-loopback binds. Local tests cover serialized ownership, live updates, failed-worker
recovery, validation, and the control API's required mutation header.

`iris-api` now holds shared serializable settings/status types and validation;
`iris-client` is the async HTTP foundation used by the CLI, independent of camera
and server dependencies. It builds for native and wasm32 browser targets. Browser
usage requires an application proxy; no browser UI was built. See the client
library README for usage.

The daemon/client pair passed Pi hardware checks: live exposure changed from
59000 us to measured 44999 us and gain from 1.5 to 2.0, without restarting capture.
Camera stop/start passed in the same daemon, and full SIGTERM daemon restart
checks also passed. The install bundle is on the Pi at `/tmp/iris-install`.
System-wide installation remains pending an interactive sudo command because
passwordless sudo is unavailable: `sudo bash /tmp/iris-install/install-irisd.sh`.
Tests leave capture stopped so the install does not conflict with a manual daemon.
Direct authenticated TCP control from the development host to `192.168.50.150:8081`
also passed status/settings/start/stop and rejected a missing credential. The
remote test daemon was stopped and its test token removed; the real installer
creates a fresh persistent credential. Validation now includes 34 native unit
checks, a client documentation example, native/ARM builds and a wasm32 client
build. The systemd unit was verified against its staged executable; activation
under the dedicated service account still needs the sudo installation step.

## Connectivity utility follow-up

`iris ping` checks the default connection; `iris ping <name>` and
`iris camera <name> ping` use a saved camera profile. The library exposes
`IrisClient::ping()`. A new authenticated `GET /ping` route responds with an empty
HTTP 204 without touching camera state. The CLI reports round-trip milliseconds
and uses a three-second timeout. Older daemon builds return 404 until updated.
The native installed CLI and `/tmp/iris-install` bundle have been refreshed.
An isolated idle Pi daemon returned pong (0.71 ms on loopback); subsequent status
remained Stopped with zero frames. The probe daemon was terminated, and no active
service was restarted. Workspace validation passed 42 tests and a client doc
example; ARM and wasm32 builds passed.

## Named client profiles follow-up

The workstation's native CLI is installed at `~/.local/bin/iris` using
`cargo install --locked --path crates/iris --bin iris --no-default-features
--features iris-client --root ~/.local`. The directory is already on PATH;
invocation from outside the repository was verified. Reinstall/update from the
repository with `just install-iris`. This does not install or start the Pi daemon.

`iris config [name] --endpoint <url> --token-file <path>` now saves native client
TOML under `$XDG_CONFIG_HOME/iris/config.toml` or `~/.config/iris/config.toml`.
`iris camera <name> start|stop|status|set` resolves the saved profile. Names are
endpoint aliases for each daemon's selected camera; there is still one capture
worker per daemon. No-argument `iris config` creates/updates the default profile.
Use `iris show` (or `iris config --show`) for offline inspection, and `--config`/`IRIS_CONFIG` for another
file. The reusable native implementation is `iris-client`'s optional `config`
module; its transport still builds for WebAssembly without filesystem access.

Config persistence is atomic, mode 0600 on Unix, stores credential paths rather
than secrets, preserves other profiles, and rejects malformed TOML rather than
overwriting it. Named CLI routing and credentials are exercised through an actual
local HTTP server. Legacy top-level commands still work using the default profile
or localhost, so older regression scripts remain compatible.
Validation for this follow-up: 39 native unit/integration tests and the client
library documentation example pass; the ARM bundle and wasm32 client build pass.
On the Pi, saving/showing a profile with an isolated temporary config passed and
its file mode was 0600. The refreshed bundle remains at `/tmp/iris-install`;
no real user config or active systemd service was changed by the profile check.

## Tagged session baseline (historical)

- Raspberry Pi 5: `192.168.50.150`, SSH user `sbeskur`.
- One IMX477 enumerated, camera index `0`.
- Iris is running from `/tmp/iris` at `0.0.0.0:8080`, 1280×720 NV12,
  requested exposure `59000 us`, analogue gain `1.5`.
- Actual metadata: exposure `58998 us`, gain approximately `1.4992679`, frame
  duration `60985 us`; valid advancing sensor timestamps and no reported errors.
- Preview: `http://192.168.50.150:8080/stream.mjpg`.
- Status: `http://192.168.50.150:8080/status`.
- Snapshot: `http://192.168.50.150:8080/frame.jpg`.
- Current test log: `/tmp/iris-quality.log`; last observed process ID was `2600`.
  Verify the command line before sending any signal; PIDs can be reused.
- `/tmp/iris` is temporary and disappears on reboot. No boot service was installed.

## Changes completed

1. Shutdown handles SIGTERM and Ctrl-C, requests capture cancellation, explicitly
   stops libcamera, and joins the worker before exit. Open MJPEG clients cannot
   block shutdown. Stop and wait for exit before starting another camera process.
2. Camera failures and worker panics update status to `Failed`. Cancelled requests,
   invalid timestamps, failed buffers, and invalid bytes-used lengths are rejected.
   Normal PiSP `Startup` buffers are recycled during a bounded warmup interval.
3. Frame freshness uses a monotonic clock with a two-second maximum age.
   Snapshots and new streams return 503 when unhealthy or stale. Existing MJPEG
   streams close on failure/expiry and never repeatedly publish the same frame.
   Image responses disable caching; status includes `frame_age_ms` and `frame_stale`.
4. Timing metadata no longer substitutes requested settings for missing actual
   exposure/gain/frame duration. These values can be null. `coi_ns` is null because
   active-array readout timing is unavailable. `first_row_coi_ns` reports the first
   row exposure midpoint when known. Timing helpers now validate inputs, return
   `Result`, and take active readout duration rather than frame period.
5. JPEG conversion handles NV12 plane stride/padding, validates negotiated pixel
   format, and selects BT.601/BT.709 and full/limited range from color metadata.
6. PiSP configuration propagates link-state errors and rejects missing required
   media links, with entity/pad diagnostics. This change lives in the libcamera
   submodule, committed as `a8589ac3` on `feature/fixes_for_pi_ubunt`; the root
   repository records that gitlink.
7. `just` build and deploy recipes share `artifact_dir`. Override with
   `IRIS_ARTIFACT_DIR` or `just artifact_dir=out/iris-quality ...`; the positional
   build-directory argument was replaced by this shared variable.

API changes: clients must accept nullable timing/exposure/gain fields. Consumers
of Rust timing helpers must handle their `Result` return type. Output-image rows
must not be treated as active sensor-array rows for rolling-shutter correction.

## Validation

- `cargo test --offline --features http-support,jpeg-support`: 24 tests pass.
- Formatting, shell syntax, root/submodule diff checks pass.
- Clippy completes with an existing unused `tracing-subscriber` dependency warning
  in the non-service feature combination.
- Iris and both examples cross-build successfully. Artifacts: `out/iris-quality/`.
- Modified libcamera cross-build succeeds. Artifacts: `out/libcamera-quality/`.
  **The new libcamera library was not installed on the Pi.** Hardware tests used
  its existing libcamera v0.7.0 installation, with the new Iris executable.
- `scripts/check-iris-restart.py` verifies advancing timestamps, healthy status,
  a JPEG response, and clean SIGTERM exit across two capture sessions. It passed
  at exposure 59000 us with gain 1.0 and again with gain 1.5.

Run the hardware regression only on an idle Pi:

```bash
python3 /tmp/check-iris-restart.py /tmp/iris --exposure-us 59000 --gain 1.5
```

## Exposure findings and next steps

- A clean-start test at exposure `75000 us`, requesting frame period `77000 us`,
  still causes a camera frontend timeout and cancelled requests on this setup.
  This is a known failing setting, not a measured maximum exposure.
- Returning to `59000 us` with the new build recovered without a power cycle;
  two subsequent sessions with gain 1.5 passed. Gain 1.5 is the validated brightness
  adjustment for now. Longer exposures need separate sensor/GMSL investigation.
- Earlier abrupt SIGTERM shutdowns with the old binary left capture failing until
  the camera was reconnected or power-cycled. Clean restarts now pass. Do not infer
  that every historical timeout was solely caused by shutdown behavior.
- Next: inspect supported exposure/frame-duration bounds and sensor mode timing,
  preserve useful kernel/libcamera diagnostics for the longer-exposure failure,
  and investigate driver/bridge behavior before another exposure sweep.
- The new PiSP link diagnostics still need hardware validation after installing
  the rebuilt libcamera. Installation requires sudo; passwordless sudo was not
  available during this session. Avoid replacing the working library casually.
- Exact frame-center CoI requires active sensor mode/line timing and crop mapping;
  do not restore the previous frame-period approximation for IMU synchronization.
- Dual-camera work remains separate. The older
  [dual-camera handoff](gmsl-dual-camera-debug-handoff.md) records vendor questions;
  no new vendor response or dual-camera validation occurred this session.

## Build and resume

```bash
export IRIS_ARTIFACT_DIR=out/iris-quality
just build
just deploy-iris
```

The libcamera artifact tarball required by the Rust cross-build is still
`out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz`. The newly built library in
`out/libcamera-quality/` is separate; choosing that library for future builds or
installation should be intentional.

On the Pi:

```bash
/tmp/iris --bind 0.0.0.0:8080 --camera-index 0 --width 1280 --height 720 \
  --exposure-us 59000 --gain 1.5
```

SSH/scp on the development host needed `-F /dev/null` because the system SSH
configuration reported bad ownership/permissions for
`/etc/ssh/ssh_config.d/20-systemd-ssh-proxy.conf`. `just` recipes still use normal
SSH configuration; fix that host configuration or use explicit SSH/scp commands.
