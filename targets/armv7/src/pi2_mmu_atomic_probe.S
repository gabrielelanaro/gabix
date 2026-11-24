    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ RESET_CONTROL, 0x3f10001c
    .equ WATCHDOG, 0x3f100024
    // Same power-manager fields as raspi2b/reset.rs::arm_deadman.
    .equ REGISTER_WRITE_KEY, 0x5a000000
    .equ WATCHDOG_FIVE_SECONDS, (5 << 16)
    .equ RESET_CONFIG_BITS, 0x30
    .equ FULL_RESET, 0x20
    .equ CPSR_MODE_MASK, 0x1f
    .equ MODE_HYP, 0x1a
    .equ MODE_SVC, 0x13
    .equ PROBE_STACK_TOP, 0x08000000
    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x3f201018
    .equ UART_TX_FULL, (1 << 5)

// Entry assumes the loader has configured UART and released only this core.
// MMU/cache state is inherited. HYP entry is converted to SVC; other modes
// are left unchanged, so they must already provide the expected privilege.
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
    ldr r0, =.Lvectors
    // VBAR selects these vectors when high vectors are disabled (inherited SCTLR.V).
    mcr p15, 0, r0, c12, c0, 0
    isb
    // Reset automatically after the diagnostic, including a stalled atomic operation.
    ldr r0, =WATCHDOG
    ldr r1, =(REGISTER_WRITE_KEY | WATCHDOG_FIVE_SECONDS)
    str r1, [r0]
    ldr r0, =RESET_CONTROL
    ldr r1, [r0]
    bic r1, r1, #RESET_CONFIG_BITS
    orr r1, r1, #FULL_RESET
    ldr r2, =REGISTER_WRITE_KEY
    orr r1, r1, r2
    str r1, [r0]
    dsb sy

    // Clear zero-initialized static storage; linker boundaries must be word-aligned.
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
    mov r2, #'R'
    b .Lexception_emit
.Lexception_undefined:
    mov r2, #'U'
    b .Lexception_emit
.Lexception_svc:
    mov r2, #'C'
    b .Lexception_emit
.Lexception_prefetch:
    mov r2, #'P'
    b .Lexception_emit
.Lexception_data:
    // Print D, raw DFSR, @, then DFAR; no stack or Rust handler is required.
    mrc p15, 0, r4, c5, c0, 0
    mrc p15, 0, r5, c6, c0, 0
    mov r2, #'D'
    bl .Lexception_putc
    mov r6, r4
    bl .Lexception_hex32
    mov r2, #'@'
    bl .Lexception_putc
    mov r6, r5
    bl .Lexception_hex32
    b .Lreset_wait
.Lexception_hyp:
    mov r2, #'H'
    b .Lexception_emit
.Lexception_irq:
    mov r2, #'I'
    b .Lexception_emit
.Lexception_fiq:
    mov r2, #'F'
.Lexception_emit:
    bl .Lexception_putc
    b .Lreset_wait
.Lexception_putc:
    ldr r0, =UART_FLAGS
.Lexception_wait:
    ldr r1, [r0]
    tst r1, #UART_TX_FULL
    bne .Lexception_wait
    ldr r0, =UART_BASE
    str r2, [r0]
    bx lr
.Lexception_hex32:
    // Emit eight high-to-low hexadecimal digits from r6; putc preserves r3/r6/r7.
    mov r7, lr
    mov r3, #8
.Lexception_hex_loop:
    mov r2, r6, lsr #28
    cmp r2, #10
    addlo r2, r2, #'0'
    addhs r2, r2, #('A' - 10)
    bl .Lexception_putc
    lsl r6, r6, #4
    subs r3, r3, #1
    bne .Lexception_hex_loop
    bx r7
    .ltorg
