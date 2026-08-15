# Compose the development command chain

Status: accepted

The Pi development workflow will remain an explicit chain of existing commands:
build the development kernel, start the single-owner UART monitor, upload the raw
image, and inspect the log. Gabix will not add a monolithic command that hides these
steps. This keeps each action observable and preserves the monitor ownership and
verification controls that were needed to make the physical UART workflow reliable.

The chain will test the loader in QEMU or an in-memory protocol harness before the
physical Pi run. The physical acceptance sequence is: flash the loader once, upload
and run kernel A, invoke `reboot()`, observe `GABIX LOADER READY`, upload and run
kernel B, then verify that a deliberately corrupted upload is rejected and retried.
