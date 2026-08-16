    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x18
    .equ UART_TX_FULL, 0x20

_start:
    cpsid if
    mrs r0, cpsr
    and r1, r0, #0x1f
    cmp r1, #0x1a
    bne .Lcheck_svc

    bic r0, r0, #0x1f
    orr r0, r0, #0x13
    adr lr, .Lcheck_svc
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

.Lcheck_svc:
    mrs r0, cpsr
    and r0, r0, #0x1f
    cmp r0, #0x13
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
