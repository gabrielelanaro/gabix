    .syntax unified
    .arm
    .section .text.boot, "ax"
    .global _start

    .equ UART_BASE, {uart_base}
    .equ GPIO_BASE, {gpio_base}
    .equ CONFIGURE_GPIO, {configure_gpio}
    .equ STACK_TOP, {stack_top}
    .equ IRQ_STACK_TOP, {irq_stack_top}
    .equ SEMIHOSTING, {semihosting}

_start:
    cpsid if

    @ Raspberry Pi starts four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #3
    bne .Lpark

    adr r0, .Lvectors
    mcr p15, 0, r0, c12, c0, 0
    isb

    cps #0x12
    ldr sp, =IRQ_STACK_TOP
    cps #0x13
    ldr sp, =STACK_TOP

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
    @ Eight data bits plus the 16-byte FIFOs. The UART userspace loader polls
    @ RX, so the FIFO absorbs short host-side bursts between reads.
    mov r0, #0x70
    str r0, [r4, #0x2c]
    ldr r0, =0x301
    str r0, [r4, #0x30]

    bl {rust_entry}

.Lexit:
    .if SEMIHOSTING
    mov r0, #0x20
    adr r1, .Lexit_block
    svc #0x123456
    .endif
    b .Lpark

.Lpark:
    wfe
    b .Lpark

    .align 5
.Lvectors:
    b .Lpark
    b .Lpark
    b .Lsvc
    b .Lpark
    b .Lpark
    b .Lpark
    b .Lirq
    b .Lpark

.Lsvc:
    stmdb sp!, {{r0-r12, lr}}
    mov r0, sp
    bl {svc_entry}
    ldmia sp!, {{r0-r12, lr}}
    movs pc, lr

.Lirq:
    sub lr, lr, #4
    stmdb sp!, {{r0-r12, lr}}
    bl {irq_entry}
    ldmia sp!, {{r0-r12, lr}}
    subs pc, lr, #0

    .align 2
.Lexit_block:
    .if SEMIHOSTING
    .word 0x20026
    .word 0
    .endif

    .ltorg
