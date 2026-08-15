# Wait for the development kernel after reset

Status: accepted

In development-loader mode, the UART loader waits indefinitely after reset for a
kernel upload. It prints a ready marker and does not fall back to an older SD-card
kernel. This keeps the host/loader handshake deterministic and prevents stale code
from being mistaken for the current experiment; a timeout-based fallback can be
added as a separate production mode later.
