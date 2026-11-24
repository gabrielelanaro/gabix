    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ RESET_CONTROL, 0x3f10001c
    .equ WATCHDOG, 0x3f100024
    // Power-manager fields match raspi2b/reset.rs.
    .equ REGISTER_WRITE_KEY, 0x5a000000
    .equ PROBE_RESET_TICKS, (15 << 16)
    .equ RESET_CONFIG_BITS, 0x30
    .equ FULL_RESET, 0x20
    .equ CPSR_MODE_MASK, 0x1f
    .equ MODE_HYP, 0x1a
    .equ MODE_SVC, 0x13
    .equ PROBE_STACK_TOP, 0x08000000

// Requires configured UART, accessible RAM and a single executing core.
// HYP entry switches to SVC; other modes and cache/MMU state are inherited.
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
    ldr sp, =PROBE_STACK_TOP
    // One countdown covers initialization, polling, UART output and the final wait.
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

    ldr r0, =__bss_start
    ldr r1, =__bss_end
    mov r2, #0
.Lzero_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lzero_bss

    bl pi2_lan9512_transmit_probe_main
.Lreset_wait:
    wfe
    b .Lreset_wait
    .ltorg
