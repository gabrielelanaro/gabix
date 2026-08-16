    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ RESET_CONTROL, 0x3f10001c
    .equ WATCHDOG, 0x3f100024
    .equ PASSWORD, 0x5a000000
    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x3f201018

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
    ldr r0, =.Lvectors
    mcr p15, 0, r0, c12, c0, 0
    isb
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

    bl pi2_mmu_atomic_probe_main
.Lreset_wait:
    wfe
    b .Lreset_wait

    .balign 32
.Lvectors:
    b .Lexception_reset
    b .Lexception_undefined
    b .Lexception_svc
    b .Lexception_prefetch
    b .Lexception_data
    b .Lexception_hyp
    b .Lexception_irq
    b .Lexception_fiq

.Lexception_reset:
    mov r2, #0x52
    b .Lexception_emit
.Lexception_undefined:
    mov r2, #0x55
    b .Lexception_emit
.Lexception_svc:
    mov r2, #0x43
    b .Lexception_emit
.Lexception_prefetch:
    mov r2, #0x50
    b .Lexception_emit
.Lexception_data:
    mrc p15, 0, r4, c5, c0, 0
    mrc p15, 0, r5, c6, c0, 0
    mov r2, #0x44
    bl .Lexception_putc
    mov r6, r4
    bl .Lexception_hex32
    mov r2, #0x40
    bl .Lexception_putc
    mov r6, r5
    bl .Lexception_hex32
    b .Lreset_wait
.Lexception_hyp:
    mov r2, #0x48
    b .Lexception_emit
.Lexception_irq:
    mov r2, #0x49
    b .Lexception_emit
.Lexception_fiq:
    mov r2, #0x46
.Lexception_emit:
    bl .Lexception_putc
    b .Lreset_wait
.Lexception_putc:
    ldr r0, =UART_FLAGS
.Lexception_wait:
    ldr r1, [r0]
    tst r1, #(1 << 5)
    bne .Lexception_wait
    ldr r0, =UART_BASE
    str r2, [r0]
    bx lr
.Lexception_hex32:
    mov r7, lr
    mov r3, #8
.Lexception_hex_loop:
    mov r2, r6, lsr #28
    cmp r2, #10
    addlo r2, r2, #0x30
    addhs r2, r2, #0x37
    bl .Lexception_putc
    lsl r6, r6, #4
    subs r3, r3, #1
    bne .Lexception_hex_loop
    bx r7
    .ltorg
