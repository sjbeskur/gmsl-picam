# Overlays

Drop camera-related overlays here when the stock Ubuntu/Raspberry Pi overlay
set is not enough.

Use one of these forms:

- `name.dts`
  Built into `name.dtbo` during `build-kernel-pack.sh`

- `name.dtbo`
  Copied directly into the kernel pack without recompilation

For Raspberry Pi camera work, using a vendor-supplied prebuilt `.dtbo` is often
the fastest route when the source overlay is not available.
