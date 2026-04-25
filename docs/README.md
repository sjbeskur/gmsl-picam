# GMSL Pi Camera Notes

This repo includes an Ubuntu-focused cross-build path for `libcamera` artifacts
that can be built in Docker and deployed to a Raspberry Pi 5, plus a matching
kernel-pack workflow for overlays and out-of-tree modules.

The `libcamera` directory is a git submodule pointing to
[sjbeskur/libcamera-gmsl](https://github.com/sjbeskur/libcamera-gmsl) which
contains GMSL-specific fixes on top of upstream libcamera.

## Cloning

```bash
git clone --recurse-submodules git@github.com:sjbeskur/gmsl-picam.git
```

If you already cloned without `--recurse-submodules`:

```bash
git submodule update --init --recursive
```

To pull the latest libcamera fixes after the submodule is updated:

```bash
git submodule update --remote libcamera
```

## Guides

- [Ubuntu arm64 cross-build guide](./ubuntu-arm64-cross-build.md)
- [Repeatable Pi 5 Ubuntu bring-up steps](./pi5-ubuntu-imx477-repeatable-steps.md)
- [Camera operations, GStreamer streaming, and debug reference](./gmsl-camera-operations.md)

Reference notes from the Arducam quick-start for the 12MP IMX477:

- Source: https://docs.arducam.com/GMSL-Camera-Solution/GMSL-Camera-for-Raspberry-Pi/Quick-Start-Guide/#software-configuration
- Raspberry Pi 5 stores boot config in `/boot/firmware/config.txt`
- Disable camera auto-detection with `camera_auto_detect=0`
- Add the IMX477 overlay under `[all]` with `dtoverlay=imx477`
- If the camera is on CAM0, use `dtoverlay=imx477,cam0`

Those settings are necessary, but on Ubuntu they are not sufficient by
themselves. The kernel, device tree, and media drivers still need to expose the
camera pipeline correctly before `libcamera` in user space can use it.
