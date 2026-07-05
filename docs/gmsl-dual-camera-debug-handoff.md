# GMSL Dual-Camera Debug Handoff

Date: 2026-07-05

## Hardware

- Raspberry Pi 5 at `192.168.50.24`
- User: `sbeskur`
- Arducam SKU reported by user: `B0550`
- Product description: `Arducam GMSL2 12MP IMX477 Raspberry Pi Camera Extension Kit, Support Up to 15-Meter Extension, Compatible with Raspberry Pi HQ Camera`

## Current Working Baseline

The Pi works with one camera using:

```ini
camera_auto_detect=0
dtoverlay=imx477,cam0
```

With this config:

- `/usr/local/bin/cam --list` enumerates one `imx477` camera.
- Kernel media graph exposes one sensor entity: `imx477 10-001a`.
- Active device tree has one camera node under RP1 I2C:
  `/proc/device-tree/axi/pcie@120000/rp1/i2c@88000/imx477@1a`

## Ruled Out

The stock Raspberry Pi `camera-mux-2port` overlay was tested:

```ini
dtoverlay=camera-mux-2port,cam0-imx477,cam1-imx477,cam0
```

Result:

- `cam --list` showed zero cameras.
- Device tree changed to a PCA-style mux topology:
  - `.../i2c@88000/pca@70/i2c@0/imx477@1a`
  - `.../i2c@88000/pca@70/i2c@1/imx477@1a`
- This is not compatible with the current GMSL setup as-is.

Conclusion: `camera-mux-2port` is not the right overlay path for this board.

## Installed Overlay Inventory

Relevant overlay search showed only generic camera/mux overlays:

```text
arducam-64mp.dtbo
arducam-pivariety.dtbo
camera-mux-2port.dtbo
camera-mux-4port.dtbo
imx477.dtbo
max98357a.dtbo
maxtherm.dtbo
tpm-slb9670.dtbo
tpm-slb9673.dtbo
```

No installed overlays or local files matched `gmsl`, `max929x`, or `max967xx`.

## Current Assessment

The missing second camera is below libcamera for now:

- The kernel/device tree only exposes one IMX477 with the working config.
- libcamera cannot enumerate a second camera until the kernel media graph exposes a second sensor or the correct multi-camera GMSL topology.
- Previous C++ libcamera GMSL patches may still matter later, but this branch is currently blocked on vendor overlay/driver information.

## Pending External Info

User contacted Arducam support.

Question sent/needed:

```text
I have Arducam SKU B0550, "GMSL2 12MP IMX477 Raspberry Pi Camera Extension Kit".
On Raspberry Pi 5 Ubuntu 24.04, dtoverlay=imx477,cam0 exposes one camera.
No GMSL/MAX929x overlay is installed.
Does B0550 support two simultaneous IMX477 cameras on one Pi receiver?
If yes, what exact overlay line or .dtbo/.dts driver package should be used?
```

When Arducam replies, validate their instructions first by checking:

```bash
/usr/local/bin/cam --list
media-ctl -d /dev/media0 -p
find /proc/device-tree/axi/pcie@120000/rp1 -maxdepth 6 -iname '*imx*' -print
sudo dmesg | grep -Ei 'imx477|camera|csi|cfe|i2c|max9|929|967|arducam|gmsl' | tail -n 180
```
