    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ GPIO_BASE, 0x3f200000
    .equ UART_BASE, 0x3f201000
    .equ STACK_TOP, 0x08000000

_start:
    cpsid if

    @ Raspberry Pi starts all four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #3
    bne .Lpark

    ldr sp, =STACK_TOP

    @ GPIO14 = PL011 TXD and GPIO15 = PL011 RXD.
    ldr r4, =GPIO_BASE
    ldr r0, [r4, #4]
    ldr r1, =0x0003f000
    bic r0, r0, r1
    ldr r1, =0x00024000
    orr r0, r0, r1
    str r0, [r4, #4]

    @ PL011 at 115200 baud with the configured 3 MHz UART clock.
    ldr r4, =UART_BASE
    mov r0, #0
    str r0, [r4, #0x30]
    ldr r0, =0x7ff
    str r0, [r4, #0x44]
    mov r0, #1
    str r0, [r4, #0x24]
    mov r0, #40
    str r0, [r4, #0x28]
    mov r0, #0x60
    str r0, [r4, #0x2c]
    ldr r0, =0x301
    str r0, [r4, #0x30]

    ldr r1, =.Lboot_message
    bl .Lwrite_string

.Lheartbeat:
    ldr r1, =.Lheartbeat_message
    bl .Lwrite_string
    mov r0, #0x40
.Ldelay_outer:
    mov r2, #0xff
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
    ldr r2, [r4, #0x18]
    tst r2, #0x20
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
