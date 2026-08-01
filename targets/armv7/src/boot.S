    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, {uart_base}
    .equ GPIO_BASE, {gpio_base}
    .equ CONFIGURE_GPIO, {configure_gpio}

_start:
    cpsid if

    @ Raspberry Pi starts four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #3
    bne .Lpark

    .if CONFIGURE_GPIO
    ldr r4, =GPIO_BASE
    ldr r0, [r4, #4]
    ldr r1, =0x0003f000
    bic r0, r0, r1
    ldr r1, =0x00024000
    orr r0, r0, r1
    str r0, [r4, #4]
    .endif

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

    adr r1, {message}
.Lwrite:
    ldrb r2, [r1], #1
    cmp r2, #0
    beq .Lexit
.Lwait_uart:
    ldr r3, [r4, #0x18]
    tst r3, #0x20
    bne .Lwait_uart
    str r2, [r4]
    b .Lwrite

.Lexit:
    mov r0, #0x20
    adr r1, .Lexit_block
    svc #0x123456

.Lpark:
    wfe
    b .Lpark

    .align 2
.Lexit_block:
    .word 0x20026
    .word 0

    .ltorg
