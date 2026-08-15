# Check the complete kernel before booting

Status: accepted

The v1 UART loader will receive a development kernel as one byte stream with a
`GABIXLOAD` marker, protocol version `1`, a 32-bit little-endian size, a 32-bit
little-endian CRC32, and raw kernel bytes. It will jump to the kernel only after both checks pass;
otherwise it will report the failure, discard the received bytes, and wait for a
fresh upload without clearing RAM. The host will retry the complete transfer when
necessary. This keeps
the protocol small while preventing interrupted or corrupted uploads from becoming
ambiguous boot failures; chunk acknowledgements can be added if measurements show
that complete-transfer retries are too costly.

After a valid upload, the loader prints `UPLOAD OK` and immediately transfers
control to the development kernel. There is no separate commit command in v1.

The loader waits indefinitely for `GABIXLOAD`. Once an upload header has been
accepted, five seconds without a payload byte produces `UPLOAD FAILED` and returns
the loader to its ready state.
