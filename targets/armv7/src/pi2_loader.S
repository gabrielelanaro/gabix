    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ GPIO_BASE, 0x3f200000
    .equ UART_BASE, 0x3f201000
    .equ STACK_TOP, 0x00080000
    .equ CORE_INDEX_MASK, 3
    .equ GPIO_FUNCTION_SELECT_1, 0x04
    .equ GPIO14_15_FUNCTION_MASK, ((7 << 12) | (7 << 15))
    .equ GPIO14_15_ALT0_UART, ((4 << 12) | (4 << 15))
    .equ UART_BAUD_INTEGER, 0x24
    .equ UART_BAUD_FRACTION, 0x28
    .equ UART_LINE_CONTROL, 0x2c
    .equ UART_CONTROL, 0x30
    .equ UART_INTERRUPT_CLEAR, 0x44
    .equ UART_CLEAR_ALL_INTERRUPTS, 0x7ff
    .equ UART_EIGHT_BITS_WITH_FIFO, ((3 << 5) | (1 << 4))
    .equ UART_ENABLE_TX_RX, ((1 << 0) | (1 << 8) | (1 << 9))
    @ 3 MHz / (16 * 115200) = 1 + 40/64 after fractional rounding.
    .equ BAUD_DIVISOR_INTEGER, 1
    .equ BAUD_DIVISOR_FRACTION, 40

@ The caller supplies usable privileged execution and identity-accessible RAM.
@ This entry leaves CPU mode, MMU, caches and vector base unchanged.
_start:
    cpsid if

    @ Raspberry Pi 2 starts all four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #CORE_INDEX_MASK
    bne .Lpark

    ldr sp, =STACK_TOP
    ldr r0, =__bss_start
    ldr r1, =__bss_end
    mov r2, #0
.Lclear_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lclear_bss

    @ GPIO14 = PL011 TXD and GPIO15 = PL011 RXD.
    ldr r4, =GPIO_BASE
    ldr r0, [r4, #GPIO_FUNCTION_SELECT_1]
    ldr r1, =GPIO14_15_FUNCTION_MASK
    bic r0, r0, r1
    ldr r1, =GPIO14_15_ALT0_UART
    orr r0, r0, r1
    str r0, [r4, #GPIO_FUNCTION_SELECT_1]

    @ boot/config.txt requests 3 MHz. These fixed divisors target 115200 baud;
    @ this loader does not query or set the source clock itself.
    ldr r4, =UART_BASE
    mov r0, #0
    str r0, [r4, #UART_CONTROL]
    ldr r0, =UART_CLEAR_ALL_INTERRUPTS
    str r0, [r4, #UART_INTERRUPT_CLEAR]
    mov r0, #BAUD_DIVISOR_INTEGER
    str r0, [r4, #UART_BAUD_INTEGER]
    mov r0, #BAUD_DIVISOR_FRACTION
    str r0, [r4, #UART_BAUD_FRACTION]
    @ Enable the 16-byte PL011 RX/TX FIFOs so the polling loader can absorb
    @ short host-side bursts without overrunning the one-byte receive path.
    mov r0, #UART_EIGHT_BITS_WITH_FIFO
    str r0, [r4, #UART_LINE_CONTROL]
    ldr r0, =UART_ENABLE_TX_RX
    str r0, [r4, #UART_CONTROL]

    bl pi2_loader_main

.Lpark:
    wfe
    b .Lpark

    .ltorg
