# Repeatable Pi 5 Ubuntu Bring-Up

This is the normalized bring-up sequence from the shell history used to get a
Raspberry Pi 5 on Ubuntu 24.04.4 LTS to the point where the kernel detects the
Arducam GMSL camera based on the 12MP IMX477 sensor and the cross-built
`libcamera` user-space bundle is installed.

Use this as the repeatable runbook.

## Scope

Tested ingredients:

- Raspberry Pi 5
- Ubuntu 24.04.4 LTS (`noble`)
- Arducam GMSL camera path presenting an `imx477` sensor
- Cross-built `libcamera` bundle from this repo

## 1. Configure The Pi Boot Overlay

Edit `/boot/firmware/config.txt`:

```bash
sudo vim /boot/firmware/config.txt
```

Required settings:

```ini
camera_auto_detect=0
dtoverlay=imx477
```

If the camera is physically connected to `CAM0`, use:

```ini
dtoverlay=imx477,cam0
```

Common mistake we hit:

```ini
dtoverlay-imx477,cam
```

That line is invalid and will not apply the overlay.

Reboot after editing:

```bash
sudo reboot
```

## 2. Verify The Kernel Side

Run these checks after reboot:

```bash
uname -r
grep -nE 'camera_auto_detect|dtoverlay' /boot/firmware/config.txt
ls /boot/firmware/overlays | grep -E '^imx477.*\.dtbo$'
ls -l /dev/media* /dev/video* 2>/dev/null
sudo dmesg | grep -Ei 'imx477|arducam|gmsl|max9|unicam|rp1|imaging'
```

Expected good signs:

- `imx477.dtbo` exists
- `/dev/media*` nodes exist
- `dmesg` includes lines like:

```text
imx477 10-001a: Device found is imx477
rp1-cfe ... Using sensor imx477 10-001a for capture
... Registered ... as /dev/video0
```

Important real-world lesson:

- Make sure the camera is actually plugged in before debugging I2C and overlays

## 3. Install Pi Debug Tools

```bash
sudo apt-get update
sudo apt-get install -y i2c-tools v4l-utils
media-ctl -p
```

Expected good sign:

- `media-ctl -p` shows `imx477 10-001a` linked into `csi2`

## 4. Build The User-Space Bundle On The Host

From the repo root on the build machine:

```bash
./scripts/build-libcamera-arm64.sh
```

Expected output:

```bash
out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz
```

Copy it to the Pi:

```bash
scp out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz ubuntu@PI_HOST:/tmp/
```

## 5. Install Runtime Dependencies On Ubuntu 24.04

On the Pi, install the runtime packages that were required in practice for this
bring-up:

```bash
sudo apt-get update
sudo apt-get install -y \
  libyaml-0-2 \
  libgnutls30 \
  libdw1 \
  libunwind8 \
  libudev1 \
  libevent-2.1-7t64 \
  libevent-pthreads-2.1-7t64 \
  libjpeg-turbo8 \
  libgstreamer1.0-0 \
  libgstreamer-plugins-base1.0-0 \
  libdrm2 \
  libexif12 \
  libtiff6 \
  zlib1g \
  libegl1 \
  libgles2
```

## 6. Install The Cross-Built Bundle

```bash
sudo tar -C / -xzf /tmp/libcamera-arm64-ubuntu.tar.gz
sudo ldconfig
```

## 7. First User-Space Checks

```bash
/usr/local/bin/cam --list
gst-inspect-1.0 libcamerasrc
```

At the point this runbook was captured, `cam` successfully started and printed:

```text
[0:..] INFO Camera camera_manager.cpp:340 libcamera v0.7.0
Available cameras:
```

That means:

- the binary and its basic shared libraries are loading
- camera enumeration in `libcamera` still needs follow-up debugging if the list
  is empty

## 8. If `cam` Fails To Start

Use this to find the next missing shared library:

```bash
ldd /usr/local/bin/cam | grep "not found"
```

During this bring-up, the missing runtime libraries were discovered in this
order:

- `libevent_pthreads-2.1.so.7`
- `libEGL.so.1`
- `libGLESv2.so.2`

Those corresponded to these packages on Ubuntu 24.04:

- `libevent-pthreads-2.1-7t64`
- `libegl1`
- `libgles2`

## 9. If `cam` Starts But Lists No Cameras

Run:

```bash
sudo /usr/local/bin/cam --list
ldd /usr/local/bin/cam | grep "not found" || true
ldd /usr/local/lib/libcamera/ipa/ipa_rpi_pisp.so | grep "not found" || true
ldd /usr/local/lib/libpisp.so.1 | grep "not found" || true
sudo env LIBCAMERA_LOG_LEVELS="RPI:DEBUG,Pipeline:DEBUG,CameraSensor:DEBUG,IPAModule:DEBUG,IPAProxy:DEBUG,MediaDevice:DEBUG" /usr/local/bin/cam --list
```

That is the next debug branch if the kernel side is healthy but libcamera still
does not register the camera in user space.

If debug logs show the PiSP pipeline loading the tuning file and then crashing,
for example:

```text
INFO RPI pisp.cpp:... libpisp version ...
INFO IPAProxy ipa_proxy.cpp:... Using tuning file /usr/local/share/libcamera/ipa/rpi/pisp/imx477.json
Segmentation fault
```

redeploy the latest user-space bundle from this repo. The Ubuntu 24.04 bring-up
required two Raspberry Pi pipeline fixes in user space:

- accept PiSP CFE entity names that use underscores instead of hyphens
- guard PiSP FE/BE `mmap()` failures instead of dereferencing `MAP_FAILED`

## 10. Headless GStreamer Streaming To A Host Machine

For a headless Raspberry Pi, the simplest first network test is JPEG-over-TCP.
These commands are adapted from the vendored
[libcamera README](../libcamera/README.rst).

First, on the Pi, make sure GStreamer can see the `libcamerasrc` plugin:

```bash
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0${GST_PLUGIN_PATH:+:$GST_PLUGIN_PATH}
gst-inspect-1.0 libcamerasrc
gst-device-monitor-1.0 Video
```

If `gst-device-monitor-1.0 Video` does not list a libcamera-backed camera, fix
camera enumeration before moving on to network streaming.

### Stream Live Video From Pi To Host

On the Pi:

```bash
export GST_PLUGIN_PATH=/usr/local/lib/gstreamer-1.0${GST_PLUGIN_PATH:+:$GST_PLUGIN_PATH}
GST_DEBUG=libcamera*:7 gst-launch-1.0 -e -v \
  libcamerasrc ! \
  'video/x-raw,colorimetry=bt709,format=NV12,width=1280,height=720,framerate=30/1' ! \
  queue ! videoconvert ! jpegenc ! multipartmux ! \
  tcpserversink host=0.0.0.0 port=5000
```

On the host:

```bash
gst-launch-1.0 -e -v \
  tcpclientsrc host=PI_HOST port=5000 ! \
  multipartdemux ! jpegdec ! autovideosink
```

Replace `PI_HOST` with the Pi hostname or IP address.

### Save JPEG Frames On The Host

On the Pi, use the same sender pipeline as above.

On the host:

```bash
gst-launch-1.0 -e -v \
  tcpclientsrc host=PI_HOST port=5000 ! \
  multipartdemux ! multifilesink location=frame-%05d.jpg
```

### Notes

- Use `1280x720` at `30/1` as the first test because it is much lighter than
  full 12MP streaming
- If the host is also headless, use `multifilesink` or `fakesink` instead of
  `autovideosink`
- If plugin discovery fails, confirm `GST_PLUGIN_PATH` includes
  `/usr/local/lib/gstreamer-1.0`
