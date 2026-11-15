    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x18
    .equ UART_TX_FULL, 0x20

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
    bne .Lcheck_svc

    bic r0, r0, #CPSR_MODE_MASK
    orr r0, r0, #MODE_SVC
    adr lr, .Lcheck_svc
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

// S reports SVC mode; E reports another mode. This does not execute an SVC instruction.
.Lcheck_svc:
    mrs r0, cpsr
    and r0, r0, #CPSR_MODE_MASK
    cmp r0, #MODE_SVC
    moveq r0, #'S'
    movne r0, #'E'
    ldr r1, =UART_BASE

.Lwait_tx:
    ldr r2, [r1, #UART_FLAGS]
    tst r2, #UART_TX_FULL
    bne .Lwait_tx
    str r0, [r1]

.Lhalt:
    wfe
    b .Lhalt

    .ltorg
