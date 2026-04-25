# Ubuntu Arm64 Cross-Build Guide

This guide is for a Raspberry Pi 5 running Ubuntu Server with an Arducam GMSL
camera based on the 12MP IMX477 sensor.

For the condensed, command-by-command runbook derived from actual shell
history, see [Repeatable Pi 5 Ubuntu bring-up steps](./pi5-ubuntu-imx477-repeatable-steps.md).

The repo now has a two-artifact workflow:

- a user-space pack for `libcamera`, `cam`, and `libcamerasrc`
- a kernel pack matched to one exact Pi kernel release for overlays and any
  out-of-tree modules

That split is deliberate. If the Pi kernel does not expose the camera media
graph correctly, user-space packages alone will not make the camera work.

## Artifact 1: User Space

The user-space build cross-compiles for `aarch64` inside a normal `x86_64`
Ubuntu Docker container. It does not rely on QEMU emulation.

It produces:

- `libcamera` shared libraries
- `cam`
- `libcamerasrc` GStreamer plugin
- Raspberry Pi pipeline handlers for `rpi/pisp` and `rpi/vc4`

Build it from the repo root:

```bash
./scripts/build-libcamera-arm64.sh
```

Output:

```bash
out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz
```

## Artifact 2: Kernel Pack

The kernel pack is built against the exact kernel build tree from the target
Pi. That is how you overcome the Ubuntu caveat safely.

The pack can contain:

- custom or prebuilt overlays from [kernel/overlays](../kernel/overlays/README.md)
- out-of-tree modules from [kernel/modules](../kernel/modules/README.md)

### Step 1: Capture The Exact Pi Kernel Tree

On the Raspberry Pi, install matching headers and package the running kernel
tree:

```bash
sudo apt-get update
sudo apt-get install -y linux-headers-$(uname -r)
./scripts/package-running-kernel-tree.sh
```

This writes a file like:

```bash
pi-kernel-$(uname -r).tar.gz
```

Copy that archive back to the build machine.

### Step 2: Build The Kernel Pack

From the repo root on the build machine:

```bash
./scripts/build-kernel-pack.sh \
  --kernel-release 6.8.0-1018-raspi \
  --kernel-tree-tar /path/to/pi-kernel-6.8.0-1018-raspi.tar.gz
```

Output:

```bash
out/kernel-pack/6.8.0-1018-raspi/kernel-pack-6.8.0-1018-raspi.tar.gz
out/kernel-pack/6.8.0-1018-raspi/kernel-pack-6.8.0-1018-raspi.manifest.txt
out/kernel-pack/6.8.0-1018-raspi/install-kernel-pack.sh
```

## Deploy To The Pi

Copy both artifacts to the Pi:

```bash
scp out/libcamera-arm64/libcamera-arm64-ubuntu.tar.gz ubuntu@PI_HOST:/tmp/
scp out/kernel-pack/6.8.0-1018-raspi/kernel-pack-6.8.0-1018-raspi.tar.gz ubuntu@PI_HOST:/tmp/
scp out/kernel-pack/6.8.0-1018-raspi/install-kernel-pack.sh ubuntu@PI_HOST:/tmp/
```

Install the user-space runtime dependencies on Ubuntu 24.04 arm64:

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

Install user space:

```bash
sudo tar -C / -xzf /tmp/libcamera-arm64-ubuntu.tar.gz
sudo ldconfig
```

Install the kernel pack:

```bash
chmod +x /tmp/install-kernel-pack.sh
/tmp/install-kernel-pack.sh /tmp/kernel-pack-6.8.0-1018-raspi.tar.gz
```

## Raspberry Pi 5 Boot Config

Arducam documents the Pi 5 camera configuration in
`/boot/firmware/config.txt`. For IMX477:

```ini
[all]
camera_auto_detect=0
dtoverlay=imx477
```

If the camera board is connected to CAM0:

```ini
dtoverlay=imx477,cam0
```

Reboot after changing boot config:

```bash
sudo reboot
```

If you add custom overlays through the kernel pack, use those overlay names in
`config.txt` instead.

## Bring-Up Checklist On The Pi

After booting the Pi, verify the kernel side first:

```bash
uname -a
cat /boot/firmware/config.txt
ls /boot/firmware/overlays | grep -E 'imx477|arducam|gmsl'
dmesg | grep -Ei 'imx477|arducam|gmsl|max9|camera|csi'
ls -l /dev/media* /dev/video* 2>/dev/null
media-ctl -p
/usr/local/bin/cam --list
gst-inspect-1.0 libcamerasrc
```

Interpretation:

- If `/dev/media*` nodes are missing, the kernel side is still not enumerating
- If media nodes exist but `/usr/local/bin/cam --list` is empty, check the
  pipeline handler, IPA data, and camera graph details
- If `/usr/local/bin/cam --list` logs the PiSP tuning file and then segfaults,
  redeploy the latest bundle from this repo before debugging GStreamer
- If `gst-inspect-1.0 libcamerasrc` fails, the plugin is not installed or a
  runtime dependency is missing

## Practical Notes

- Use the stock `imx477` overlay first if Ubuntu already provides it
- If the vendor gives you a prebuilt `.dtbo`, drop it into `kernel/overlays/`
  and rebuild the kernel pack
- If the vendor gives you a driver source tree, put it under `kernel/modules/`
  and rebuild the kernel pack against the exact Pi kernel archive
- If the vendor only supports Raspberry Pi OS kernel packages and provides no
  source or compatible Ubuntu binaries, the practical fallback is a
  Raspberry Pi-aligned kernel with Ubuntu user space
