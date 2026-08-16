    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ RESET_CONTROL, 0x3f10001c
    .equ WATCHDOG, 0x3f100024
    .equ PASSWORD, 0x5a000000

_start:
    cpsid if
    mrs r0, cpsr
    and r1, r0, #0x1f
    cmp r1, #0x1a
    bne .Lsvc_entry
    bic r0, r0, #0x1f
    orr r0, r0, #0x13
    adr lr, .Lsvc_entry
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

.Lsvc_entry:
    ldr sp, =0x08000000
    ldr r0, =WATCHDOG
    ldr r1, =(PASSWORD | 0x50000)
    str r1, [r0]
    ldr r0, =RESET_CONTROL
    ldr r1, [r0]
    bic r1, r1, #0x30
    orr r1, r1, #0x20
    ldr r2, =PASSWORD
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

    bl pi2_atomic_probe_main
.Lreset_wait:
    wfe
    b .Lreset_wait
    .ltorg
