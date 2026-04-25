# GMSL Camera Operations Guide

End-to-end reference for the Arducam GMSL IMX477 camera on a Raspberry Pi 5 running
Ubuntu 24.04 with the cross-built libcamera bundle from this repo.

---

## Quick Start

### Capture frames to disk

```bash
sudo /usr/local/bin/cam -c 1 --capture=5 --file=/tmp/frame-#.raw
```

Expected output:

```
INFO Camera camera_manager.cpp:340 libcamera v0.7.0
WARN RPI pisp.cpp:... IPA indicates embedded data support but sensor reports no embedded format; disabling embedded data
INFO RPI pisp.cpp:... Registered camera .../imx477@1a ...
INFO RPI pisp.cpp:... Sensor: ... - Selected sensor format: 2028x1520-SBGGR12_1X12/RAW ...
cam0: Capture 5 frames
... (0.00 fps) cam0-stream0 seq: 000000 bytesused: 1920000
... (30.x fps) cam0-stream0 seq: 000001 bytesused: 1920000
...
```

The `disabling embedded data` warning is normal for the GMSL bridge path — embedded
data does not flow through the MAX9296A deserializer.

### Verify frame content

```bash
# Check file sizes (~1.9 MB each for 800x600 XRGB8888)
ls -lh /tmp/frame-*.raw

# Confirm non-zero pixel data
xxd /tmp/frame-0.raw | head -5
```

---

## GStreamer Live Streaming

### Install GStreamer on the host machine

**Ubuntu / Debian:**

```bash
sudo apt-get install -y \
  gstreamer1.0-tools \
  gstreamer1.0-plugins-good \
  gstreamer1.0-plugins-base \
  gstreamer1.0-gtk3
```

**macOS (Homebrew):**

```bash
brew install gstreamer gst-plugins-good gst-plugins-base
```

### Stream live video from Pi to host

First, confirm the Pi GStreamer plugin sees the camera:

```bash
# On Pi
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0
gst-inspect-1.0 libcamerasrc
gst-device-monitor-1.0 Video
```

`gst-device-monitor-1.0 Video` must list a libcamera-backed device before
streaming will work.

**Pi sender** (replace `0.0.0.0` to bind only a specific interface if needed):

```bash
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0
gst-launch-1.0 -e -v \
  libcamerasrc ! \
  'video/x-raw,colorimetry=bt709,format=NV12,width=1280,height=720,framerate=30/1' ! \
  queue ! videoconvert ! jpegenc ! multipartmux ! \
  tcpserversink host=0.0.0.0 port=5000
```

**Host receiver — display in window:**

```bash
gst-launch-1.0 -e -v \
  tcpclientsrc host=192.168.50.24 port=5000 ! \
  multipartdemux ! jpegdec ! autovideosink
```

**Host receiver — save frames to disk:**

```bash
gst-launch-1.0 -e -v \
  tcpclientsrc host=192.168.50.24 port=5000 ! \
  multipartdemux ! multifilesink location=frame-%05d.jpg
```

### Notes

- Start with `1280x720` at `30/1` — it is much lighter than full 12MP streaming.
- If `autovideosink` fails on a headless host, use `multifilesink` or `fakesink`.
- If `gst-launch-1.0` cannot find `libcamerasrc`, confirm
  `GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0` is exported before running.
- Port 5000 must be reachable from the host. Check with `nc -zv 192.168.50.24 5000`.

---

## Debug Command Reference

### Camera enumeration

```bash
# List cameras visible to libcamera
/usr/local/bin/cam --list

# Same with verbose pipeline logging
sudo env LIBCAMERA_LOG_LEVELS="RPI:DEBUG,Pipeline:DEBUG,CameraSensor:DEBUG,IPAModule:DEBUG,IPAProxy:DEBUG,MediaDevice:DEBUG" \
  /usr/local/bin/cam --list

# Check the GStreamer plugin is installed and loadable
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0
gst-inspect-1.0 libcamerasrc
```

### Kernel / device node mapping

```bash
# Which driver owns which /dev/video* nodes
v4l2-ctl --list-devices

# Full media topology: rp1-cfe (camera front-end)
sudo media-ctl -d /dev/media1 -p

# Full media topology: pispbe (ISP back-end)
sudo media-ctl -d /dev/media0 -p
```

The rp1-cfe topology shows the active link state.  After a successful `cam` run
`csi2 pad4 → pisp-fe pad0` should show `[ENABLED]`.

### Link state inspection

```bash
# See all links and whether they are enabled/disabled
sudo media-ctl -d /dev/media1 -p 2>&1 | grep -E "ENABLED|DISABLED|\->"

# Confirm CSI2 → pisp-fe link is enabled after configure
sudo media-ctl -d /dev/media1 -p 2>&1 | grep "pisp-fe"
```

### dmesg — kernel-side camera health

```bash
# Did the sensor and CSI2 come up at boot?
sudo dmesg | grep -Ei 'imx477|arducam|gmsl|max9|unicam|rp1|cfe|imaging'

# Stream of new dmesg as you test
sudo dmesg -w | grep -Ei 'imx477|cfe|rp1'
```

Expected good lines:

```
imx477 10-001a: Device found is imx477
rp1-cfe ...: Using sensor imx477 10-001a for capture
... Registered ... as /dev/video4
```

### Missing shared libraries

```bash
# Check cam binary
ldd /usr/local/bin/cam | grep "not found"

# Check the PiSP IPA module
ldd /usr/local/lib/libcamera/ipa/ipa_rpi_pisp.so | grep "not found"

# Check libpisp
ldd /usr/local/lib/libpisp.so.1 | grep "not found"
```

If any are missing, install the runtime package listed in
[ubuntu-arm64-cross-build.md](./ubuntu-arm64-cross-build.md).

### V4L2 format and control inspection

```bash
# What formats does the CFE image node support?
v4l2-ctl -d /dev/video4 --list-formats-ext

# Current sensor controls (exposure, gain, etc.)
v4l2-ctl --all -d /dev/v4l-subdev2
```

---

## Deploying a New Build

```bash
# On build machine
./scripts/build-libcamera-arm64.sh

# Copy to Pi
scp out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz sbeskur@192.168.50.24:/tmp/

# On Pi — extract and refresh the linker cache
sudo tar -C / -xzf /tmp/libcamera-arm64-ubuntu.tar.gz
sudo ldconfig
```

---

## libcamera GMSL Patch Summary

The upstream libcamera PiSP pipeline handler assumes a standard Raspberry Pi 5
kernel topology. The GMSL bridge introduces several deviations that required fixes
in `libcamera/src/libcamera/pipeline/rpi/pisp/pisp.cpp` and
`libcamera/src/libcamera/pipeline/rpi/common/pipeline_base.cpp`.

### 1 — Entity name variants (underscore vs hyphen)

**Problem:** The Ubuntu GMSL kernel exposes PiSP frontend entities with underscores
(`rp1-cfe-fe_image0`) instead of hyphens (`rp1-cfe-fe-image0`).  The upstream
`DeviceMatch` and `getEntityByName` calls used only the hyphen form, causing null
entity dereferences and immediate segfaults.

**Fix:** `match()` tries both naming schemes.  `platformRegister()` uses a
`getCfeFeEntity()` helper that falls back to the underscore form if the hyphen form
returns null.

### 2 — Embedded entity renamed

**Problem:** The embedded data capture entity was renamed from `rp1-cfe-csi2-ch1`
to `rp1-cfe-embedded` in some kernel versions.

**Fix:** Two-step lookup: try `rp1-cfe-csi2-ch1` first, then `rp1-cfe-embedded`.

### 3 — Embedded data not forwarded by GMSL bridge

**Problem:** The IMX477 tuning file sets `sensorMetadata = true`, but the MAX9296A
GMSL deserializer does not forward the sensor's embedded data stream.  Without
intervention the pipeline tries to open and stream the embedded video device, which
fails with `ENOLINK` because the media link is permanently severed in the GMSL
topology.

**Fix:** In `platformRegister()`, before the embedded stream is added to
`data->streams_`, check `sensor_->embeddedDataFormat().code`.  If the code is zero
(no embedded format), clear `sensorMetadata_` so the embedded device is never opened
or streamed.  A secondary guard in `platformConfigure()` catches any late divergence.

### 4 — Empty pad link vectors

**Problem:** The CSI2 meta source pad and some pisp-fe pads have no links in the
GMSL topology.  Direct `links()[0]` access on these pads caused segfaults.

**Fix:** All `links()[0]` accesses are guarded by a `setFirstLink()` helper that
checks `pads().size()` and `links().empty()` before acting.

### 5 — CSI2 source pad index wrong for GMSL kernel

**Problem (root cause of ENOLINK / EPIPE):** The code assumed the CSI2 subdevice
always has three pads: sink(0), video source(1), meta source(2).  The GMSL kernel
exposes eight pads — four sinks (one per input channel) and four sources.  The image
source is pad 4, not pad 1.  With `csiVideoSourcePad = 1` hardcoded, the code was
iterating a sink pad's incoming links, never found `pisp-fe`, and so the
`csi2:4 → pisp-fe` link was never enabled — the root cause of every `ENOLINK` seen
on `CFE Image`.

**Fix:** `configureEntities()` now discovers the primary video source pad
dynamically: it iterates all source pads and identifies the primary one as the first
that carries a link to `rp1-cfe-csi2_ch0` (the bypass capture node).  That pad also
carries the pisp-fe link and is the only one on which the pisp-fe link is enabled.
The discovered pad index is then used for routing and `setFormat` calls.

### 6 — setRouting tolerance

**Problem:** With the correct source pad (4) used in `setRouting`, the call
succeeded and set up the routing table.  Previously, with the wrong pad (1), the call
was failing silently (error swallowed).  Blanket error-tolerance masked this
completely.

**Fix:** `setRouting` errors are only suppressed for `ENOTTY` (routing not
implemented by the entity).  Any other error — including the `-EINVAL` from the
wrong pad that was previously silent — now propagates and causes configure to fail
with a visible error message.

### 7 — configureEntities() return value not checked

**Problem:** `platformConfigure()` called `configureEntities()` without checking its
return value.  Any failure inside (wrong pad routing, format rejection) was silently
discarded and configure appeared to succeed, only for STREAMON to fail later with a
confusing error.

**Fix:** `platformConfigure()` now checks and propagates the return value.

### 8 — Stream name logged on STREAMON failure

**Fix:** `pipeline_base.cpp` `start()` loop now logs the libcamera stream name and
device node when `streamOn()` fails, making future failures immediately identifiable
without requiring `media-ctl` investigation.
