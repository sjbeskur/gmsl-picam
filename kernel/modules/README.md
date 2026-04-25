# External Modules

Each direct child directory under this folder is treated as one out-of-tree
kernel module project.

Example layout:

```text
kernel/modules/
  my-camera-bridge/
    Makefile
    my_camera_bridge.c
```

The build invokes kbuild like this:

```text
make -C <target-kernel-build-tree> M=<module-dir> ARCH=arm64 CROSS_COMPILE=aarch64-linux-gnu- modules
```

Use this for vendor bridge, serializer, deserializer, or helper drivers that
must match the exact Ubuntu kernel running on the Pi.
