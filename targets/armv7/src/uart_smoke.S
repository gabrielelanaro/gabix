    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ GPIO_BASE, 0x3f200000
    .equ UART_BASE, 0x3f201000
    .equ STACK_TOP, 0x08000000
    .equ GPIO_FUNCTION_SELECT_1, 4
    .equ GPIO14_15_FUNCTION_MASK, ((7 << 12) | (7 << 15))
    .equ GPIO14_15_ALT0, ((4 << 12) | (4 << 15))
    .equ UART_FLAGS, 0x18
    .equ UART_INTEGER_DIVISOR, 0x24
    .equ UART_FRACTIONAL_DIVISOR, 0x28
    .equ UART_LINE_CONTROL, 0x2c
    .equ UART_CONTROL, 0x30
    .equ UART_INTERRUPT_CLEAR, 0x44
    .equ UART_ALL_INTERRUPTS, 0x7ff
    .equ UART_TX_FULL, (1 << 5)
    .equ UART_EIGHT_DATA_BITS, (3 << 5)
    .equ UART_ENABLE_TX_RX, ((1 << 0) | (1 << 8) | (1 << 9))
    .equ BAUD_INTEGER_3MHZ_115200, 1
    .equ BAUD_FRACTION_3MHZ_115200, 40
    .equ CORE_INDEX_MASK, 3
    .equ HEARTBEAT_OUTER_ITERATIONS, 0x40
    .equ HEARTBEAT_INNER_ITERATIONS, 0xff

_start:
    cpsid if

    @ Keep only core 0 executing if secondary cores enter this payload.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #CORE_INDEX_MASK
    bne .Lpark

    ldr sp, =STACK_TOP

    @ GPIO14 = PL011 TXD and GPIO15 = PL011 RXD.
    ldr r4, =GPIO_BASE
    ldr r0, [r4, #GPIO_FUNCTION_SELECT_1]
    ldr r1, =GPIO14_15_FUNCTION_MASK
    bic r0, r0, r1
    ldr r1, =GPIO14_15_ALT0
    orr r0, r0, r1
    str r0, [r4, #GPIO_FUNCTION_SELECT_1]

    @ Divisors target 115200 baud assuming a 3 MHz input clock; this code does not query it.
    @ Disable UART, clear interrupts, set baud and 8-bit words, then enable TX/RX.
    @ FIFO enable remains clear in this line-control value.
    ldr r4, =UART_BASE
    mov r0, #0
    str r0, [r4, #UART_CONTROL]
    ldr r0, =UART_ALL_INTERRUPTS
    str r0, [r4, #UART_INTERRUPT_CLEAR]
    mov r0, #BAUD_INTEGER_3MHZ_115200
    str r0, [r4, #UART_INTEGER_DIVISOR]
    mov r0, #BAUD_FRACTION_3MHZ_115200
    str r0, [r4, #UART_FRACTIONAL_DIVISOR]
    mov r0, #UART_EIGHT_DATA_BITS
    str r0, [r4, #UART_LINE_CONTROL]
    ldr r0, =UART_ENABLE_TX_RX
    str r0, [r4, #UART_CONTROL]

    ldr r1, =.Lboot_message
    bl .Lwrite_string

.Lheartbeat:
    @ Busy-loop pacing depends on CPU speed; these counts are not time units.
    ldr r1, =.Lheartbeat_message
    bl .Lwrite_string
    mov r0, #HEARTBEAT_OUTER_ITERATIONS
.Ldelay_outer:
    mov r2, #HEARTBEAT_INNER_ITERATIONS
.Ldelay_inner:
    subs r2, r2, #1
    bne .Ldelay_inner
    subs r0, r0, #1
    bne .Ldelay_outer
    b .Lheartbeat

.Lwrite_string:
    ldrb r0, [r1], #1
    cmp r0, #0
    beq .Lwrite_done
.Lwait_tx:
    ldr r2, [r4, #UART_FLAGS]
    tst r2, #UART_TX_FULL
    bne .Lwait_tx
    str r0, [r4]
    b .Lwrite_string
.Lwrite_done:
    bx lr

.Lpark:
    wfe
    b .Lpark

    .align 2
.Lboot_message:
    .asciz "gabix uart smoke booted\r\n"
.Lheartbeat_message:
    .asciz "gabix uart smoke alive\r\n"

    .ltorg
