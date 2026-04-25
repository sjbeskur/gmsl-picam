# Kernel Pack Layout

This directory holds the kernel-side inputs for a Raspberry Pi deployment pack.

The build produces a tarball matched to one exact target kernel release, such
as `6.8.0-1018-raspi`.

## Directories

- `overlays/`
  Put custom camera overlays here.
  Supported inputs:
  - `*.dts`: compiled into `*.dtbo` during the pack build
  - `*.dtbo`: copied into the output pack as-is

- `modules/`
  Put out-of-tree kernel modules here.
  Each direct child directory should be a standard kbuild module with a
  `Makefile` or `Kbuild`.

## Output

The kernel pack installs files into these target paths:

- `/boot/firmware/overlays/*.dtbo`
- `/lib/modules/<kernel-release>/extra/*.ko`

The build also emits a manifest so you can see exactly what was packaged.
