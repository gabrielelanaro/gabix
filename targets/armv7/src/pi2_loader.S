    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ GPIO_BASE, 0x3f200000
    .equ UART_BASE, 0x3f201000
    .equ STACK_TOP, 0x00080000

_start:
    cpsid if

    @ Raspberry Pi 2 starts all four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #3
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
    @ Enable the 16-byte PL011 RX/TX FIFOs so the polling loader can absorb
    @ short host-side bursts without overrunning the one-byte receive path.
    mov r0, #0x70
    str r0, [r4, #0x2c]
    ldr r0, =0x301
    str r0, [r4, #0x30]

    bl pi2_loader_main

.Lpark:
    wfe
    b .Lpark

    .ltorg
