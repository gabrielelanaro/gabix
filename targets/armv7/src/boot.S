    .syntax unified
    .arm
    .arch armv7-a
    .arch_extension virt
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

    @ Pi 2 firmware and the resident UART loader can enter the kernel in Hyp
    @ mode. Normalize to SVC before programming VBAR or selecting banked
    @ exception stacks.
    mrs r0, cpsr
    and r1, r0, #0x1f
    cmp r1, #0x1a
    bne .Lsvc_entry
    bic r0, r0, #0x1f
    orr r0, r0, #0x13
    adr lr, .Lsvc_entry
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

.Lsvc_entry:

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

    ldr r0, =__bss_start
    ldr r1, =__bss_end
    mov r2, #0
.Lzero_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lzero_bss

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
    b .Lundefined
    b .Lsvc
    b .Lprefetch_abort
    b .Ldata_abort
    b .Lpark
    b .Lirq
    b .Lpark

.Lundefined:
    mov r2, #0x55
    bl .Lfault_putc
    b .Lpark

.Lprefetch_abort:
    mrc p15, 0, r4, c5, c0, 1
    mrc p15, 0, r5, c6, c0, 2
    mov r2, #0x50
    bl .Lfault_putc
    mov r6, r4
    bl .Lfault_hex32
    mov r2, #0x40
    bl .Lfault_putc
    mov r6, r5
    bl .Lfault_hex32
    b .Lpark

.Ldata_abort:
    mrc p15, 0, r4, c5, c0, 0
    mrc p15, 0, r5, c6, c0, 0
    mov r2, #0x44
    bl .Lfault_putc
    mov r6, r4
    bl .Lfault_hex32
    mov r2, #0x40
    bl .Lfault_putc
    mov r6, r5
    bl .Lfault_hex32
    b .Lpark

.Lfault_putc:
    ldr r0, =UART_BASE
.Lfault_wait:
    ldr r1, [r0, #0x18]
    tst r1, #(1 << 5)
    bne .Lfault_wait
    str r2, [r0]
    bx lr

.Lfault_hex32:
    mov r7, lr
    mov r3, #8
.Lfault_hex_loop:
    mov r2, r6, lsr #28
    cmp r2, #10
    addlo r2, r2, #0x30
    addhs r2, r2, #0x37
    bl .Lfault_putc
    lsl r6, r6, #4
    subs r3, r3, #1
    bne .Lfault_hex_loop
    bx r7

.Lsvc:
    stmdb sp!, {{r0-r12, lr}}
    mov r0, sp
    bl {svc_entry}
    ldmia sp!, {{r0-r12, lr}}
    movs pc, lr

.Lirq:
    @ Save the interrupted PC and CPSR on the current SVC (kernel) stack.
    @ Each scheduled kernel task has its own SVC stack. The IRQ bank is free
    @ again before we schedule another task, so its next IRQ cannot overwrite
    @ this task's saved return state.
    sub lr, lr, #4
    srsdb sp!, #0x13
    cps #0x13
    stmdb sp!, {{r0-r12, lr}}
    @ Keep the SVC saved status too: an IRQ may interrupt a syscall handler.
    mrs r0, spsr
    push {{r0, r1}}
    @ An interrupted instruction may have a four-byte-aligned SP. Rust calls
    @ require eight-byte alignment. r4 preserves the unrounded frame address.
    mov r4, sp
    bic sp, sp, #7
    bl {irq_entry}
    mov sp, r4
    pop {{r0, r1}}
    msr spsr_cxsf, r0
    ldmia sp!, {{r0-r12, lr}}
    rfeia sp!

    .align 2
.Lexit_block:
    .if SEMIHOSTING
    .word 0x20026
    .word 0
    .endif

    .ltorg
