    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, 0x3f201000
    .equ UART_FLAGS, 0x18
    .equ UART_TX_FULL, 0x20

_start:
    ldr r1, =UART_BASE
    mov r0, #'P'

.Lwait_tx:
    ldr r2, [r1, #UART_FLAGS]
    tst r2, #UART_TX_FULL
    bne .Lwait_tx

    str r0, [r1]
    bx lr

    .ltorg
