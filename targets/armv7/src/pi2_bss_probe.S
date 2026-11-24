    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x18
    .equ UART_TX_FULL, 0x20
    .equ UART_BUSY, 0x08
    .equ RESET_CONTROL, 0x3f10001c
    .equ WATCHDOG, 0x3f100024
    // Power-manager fields match raspi2b/reset.rs; countdown uses 65536 ticks/s.
    .equ REGISTER_WRITE_KEY, 0x5a000000
    .equ PROBE_RESET_TICKS, 10
    .equ RESET_CONFIG_BITS, 0x30
    .equ FULL_RESET, 0x20

    .equ CPSR_MODE_MASK, 0x1f
    .equ MODE_HYP, 0x1a
    .equ MODE_SVC, 0x13

// Requires configured UART, accessible probe RAM and a single executing core.
// HYP entry switches to SVC; all other incoming modes are preserved.
_start:
    cpsid if
    mrs r0, cpsr
    and r1, r0, #CPSR_MODE_MASK
    cmp r1, #MODE_HYP
    bne .Lsvc_entry
    bic r0, r0, #CPSR_MODE_MASK
    orr r0, r0, #MODE_SVC
    adr lr, .Lsvc_entry
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

.Lsvc_entry:
    @ Poison the NOBITS region so success proves that this payload cleared it.
    ldr r0, =bss_probe
    ldr r1, =bss_probe_end
    mvn r2, #0
.Lpoison_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lpoison_bss

    ldr r0, =bss_probe
    mov r2, #0
.Lzero_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lzero_bss

    ldr r0, =bss_probe
.Lcheck_bss:
    cmp r0, r1
    beq .Lbss_ok
    ldr r2, [r0], #4
    cmp r2, #0
    beq .Lcheck_bss
    mov r0, #'E'
    b .Lwrite_result

.Lbss_ok:
    mov r0, #'B'

.Lwrite_result:
    ldr r1, =UART_BASE
.Lwait_tx:
    ldr r2, [r1, #UART_FLAGS]
    tst r2, #UART_TX_FULL
    bne .Lwait_tx
    str r0, [r1]

    @ Let PL011 finish the marker, then request a full watchdog reset. The SD
    @ loader will regain control and emit GABIX LOADER READY for the next stage.
.Lwait_uart_idle:
    ldr r2, [r1, #UART_FLAGS]
    tst r2, #UART_BUSY
    bne .Lwait_uart_idle

    ldr r0, =WATCHDOG
    ldr r1, =(REGISTER_WRITE_KEY | PROBE_RESET_TICKS)
    str r1, [r0]
    ldr r0, =RESET_CONTROL
    ldr r1, [r0]
    bic r1, r1, #RESET_CONFIG_BITS
    orr r1, r1, #FULL_RESET
    ldr r2, =REGISTER_WRITE_KEY
    orr r1, r1, r2
    str r1, [r0]
    dsb sy

.Lreset_wait:
    wfe
    b .Lreset_wait

    .ltorg

    .section .bss, "aw", %nobits
    .align 4
bss_probe:
    .space 64 // Sixteen words are poisoned, cleared and checked.
bss_probe_end:
