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

    .equ CPSR_MODE_MASK, 0x1f
    .equ MODE_HYPERVISOR, 0x1a
    .equ MODE_SUPERVISOR, 0x13
    .equ MODE_IRQ, 0x12
    .equ CORE_INDEX_MASK, 3
    .equ IRQ_RETURN_BIAS, 4
    .equ PREFETCH_ABORT_RETURN_BIAS, 4
    .equ DATA_ABORT_RETURN_BIAS, 8
    @ SRS stores return PC/status, then push stores r0-r12 and LR (14 words).
    .equ ABORT_SAVED_STATUS_OFFSET, (14 * 4 + 4)
    .equ CALL_STACK_ALIGNMENT_MASK, 7
    .equ FAULT_IS_INSTRUCTION, 1
    .equ FAULT_IS_DATA, 0

    .equ GPIO_FUNCTION_SELECT_1, 0x04
    .equ GPIO14_15_FUNCTION_MASK, ((7 << 12) | (7 << 15))
    .equ GPIO14_15_ALT0_UART, ((4 << 12) | (4 << 15))
    .equ UART_FLAGS, 0x18
    .equ UART_BAUD_INTEGER, 0x24
    .equ UART_BAUD_FRACTION, 0x28
    .equ UART_LINE_CONTROL, 0x2c
    .equ UART_CONTROL, 0x30
    .equ UART_INTERRUPT_CLEAR, 0x44
    .equ UART_CLEAR_ALL_INTERRUPTS, 0x7ff
    .equ UART_TX_FIFO_FULL, (1 << 5)
    .equ UART_EIGHT_BITS_WITH_FIFO, ((3 << 5) | (1 << 4))
    .equ UART_ENABLE_TX_RX, ((1 << 0) | (1 << 8) | (1 << 9))
    @ Preserved boot divisors; the effective baud rate depends on UARTCLK.
    .equ BOOT_BAUD_INTEGER, 1
    .equ BOOT_BAUD_FRACTION, 40
    .equ SEMIHOST_EXIT_EXTENDED, 0x20
    .equ SEMIHOST_SVC, 0x123456
    .equ SEMIHOST_APPLICATION_EXIT, 0x20026

_start:
    cpsid if

    @ Pi 2 firmware and the resident UART loader can enter the kernel in Hyp
    @ mode. Normalize to SVC before programming VBAR or selecting banked
    @ exception stacks.
    mrs r0, cpsr
    and r1, r0, #CPSR_MODE_MASK
    cmp r1, #MODE_HYPERVISOR
    bne .Lsvc_entry
    bic r0, r0, #CPSR_MODE_MASK
    orr r0, r0, #MODE_SUPERVISOR
    adr lr, .Lsvc_entry
    msr spsr_hyp, r0
    msr elr_hyp, lr
    eret

.Lsvc_entry:

    @ Raspberry Pi starts four cores at the same entry point.
    mrc p15, 0, r0, c0, c0, 5
    ands r0, r0, #CORE_INDEX_MASK
    bne .Lpark

    adr r0, .Lvectors
    mcr p15, 0, r0, c12, c0, 0
    isb

    cps #MODE_IRQ
    ldr sp, =IRQ_STACK_TOP
    cps #MODE_SUPERVISOR
    ldr sp, =STACK_TOP

    ldr r0, =__bss_start
    ldr r1, =__bss_end
    mov r2, #0
.Lzero_bss:
    cmp r0, r1
    strlo r2, [r0], #4
    blo .Lzero_bss

    @ Route GPIO14/15 to UART TXD0/RXD0 using alternate function 0.
    .if CONFIGURE_GPIO
    ldr r4, =GPIO_BASE
    ldr r0, [r4, #GPIO_FUNCTION_SELECT_1]
    ldr r1, =GPIO14_15_FUNCTION_MASK
    bic r0, r0, r1
    ldr r1, =GPIO14_15_ALT0_UART
    orr r0, r0, r1
    str r0, [r4, #GPIO_FUNCTION_SELECT_1]
    .endif

    @ Disable UART, clear pending interrupt flags, then set baud/line format.
    ldr r4, =UART_BASE
    mov r0, #0
    str r0, [r4, #UART_CONTROL]
    ldr r0, =UART_CLEAR_ALL_INTERRUPTS
    str r0, [r4, #UART_INTERRUPT_CLEAR]
    mov r0, #BOOT_BAUD_INTEGER
    str r0, [r4, #UART_BAUD_INTEGER]
    mov r0, #BOOT_BAUD_FRACTION
    str r0, [r4, #UART_BAUD_FRACTION]
    @ Eight data bits plus the 16-byte FIFOs. The UART userspace loader polls
    @ RX, so the FIFO absorbs short host-side bursts between reads.
    mov r0, #UART_EIGHT_BITS_WITH_FIFO
    str r0, [r4, #UART_LINE_CONTROL]
    ldr r0, =UART_ENABLE_TX_RX
    str r0, [r4, #UART_CONTROL]

    bl {rust_entry}

.Lexit:
    .if SEMIHOSTING
    mov r0, #SEMIHOST_EXIT_EXTENDED
    adr r1, .Lexit_block
    svc #SEMIHOST_SVC
    .endif
    b .Lpark

.Lpark:
    wfe
    b .Lpark

    @ These wrappers preserve integer state only; VFP/NEON state is not saved.
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

@ Abort handlers never resume the saved frame: Rust ends the user process or
@ halts for a kernel fault. Rounding SP down therefore needs no restoration.
.Lprefetch_abort:
    cpsid i
    sub lr, lr, #PREFETCH_ABORT_RETURN_BIAS
    srsdb sp!, #MODE_SUPERVISOR
    cps #MODE_SUPERVISOR
    push {{r0-r12, lr}}
    mrc p15, 0, r0, c6, c0, 2    @ IFAR: rejected instruction address
    mrc p15, 0, r1, c5, c0, 1    @ IFSR: instruction fault kind
    ldr r2, [sp, #ABORT_SAVED_STATUS_OFFSET]
    mov r3, #FAULT_IS_INSTRUCTION
    bic sp, sp, #CALL_STACK_ALIGNMENT_MASK
    bl {memory_abort_entry}
    b .Lpark

.Ldata_abort:
    @ Abort mode has a banked return address/status. Store them directly on
    @ the faulting task's SVC stack; no shared abort stack is needed.
    cpsid i
    sub lr, lr, #DATA_ABORT_RETURN_BIAS
    srsdb sp!, #MODE_SUPERVISOR
    cps #MODE_SUPERVISOR
    push {{r0-r12, lr}}
    mrc p15, 0, r0, c6, c0, 0    @ DFAR: address the process tried to access
    mrc p15, 0, r1, c5, c0, 0    @ DFSR: fault kind and read/write bit
    ldr r2, [sp, #ABORT_SAVED_STATUS_OFFSET]            @ saved CPSR after 14 regs and return PC
    bic sp, sp, #CALL_STACK_ALIGNMENT_MASK
    mov r3, #FAULT_IS_DATA
    bl {memory_abort_entry}        @ terminates user process; kernel fault halts
    b .Lpark

.Lfault_putc:
    ldr r0, =UART_BASE
.Lfault_wait:
    ldr r1, [r0, #UART_FLAGS]
    tst r1, #UART_TX_FIFO_FULL
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
    @ The 14-word layout is syscall::UserFrame. Its saved r0 is the return value.
    @ SPSR_svc remains banked; this wrapper assumes an aligned SVC stack and
    @ no nested SVC. IRQ entry below preserves SPSR_svc across preemption.
    stmdb sp!, {{r0-r12, lr}}
    mov r0, sp
    bl {svc_entry}
    ldmia sp!, {{r0-r12, lr}}
    @ Restore PC from LR and processor status from SPSR_svc together.
    movs pc, lr

.Lirq:
    @ Save the interrupted PC and CPSR on the current SVC (kernel) stack.
    @ Each scheduled kernel task has its own SVC stack. The IRQ bank is free
    @ again before we schedule another task, so its next IRQ cannot overwrite
    @ this task's saved return state.
    sub lr, lr, #IRQ_RETURN_BIAS
    srsdb sp!, #MODE_SUPERVISOR
    cps #MODE_SUPERVISOR
    stmdb sp!, {{r0-r12, lr}}
    @ Keep the SVC saved status too: an IRQ may interrupt a syscall handler.
    mrs r0, spsr
    push {{r0, r1}}
    @ An interrupted instruction may have a four-byte-aligned SP. Rust calls
    @ require eight-byte alignment. r4 preserves the unrounded frame address.
    mov r4, sp
    bic sp, sp, #CALL_STACK_ALIGNMENT_MASK
    bl {irq_entry}
    mov sp, r4
    pop {{r0, r1}}
    msr spsr_cxsf, r0
    ldmia sp!, {{r0-r12, lr}}
    @ SRS's saved PC/status pair restores the interrupted mode and IRQ mask.
    rfeia sp!

    .align 2
.Lexit_block:
    .if SEMIHOSTING
    .word SEMIHOST_APPLICATION_EXIT
    .word 0
    .endif

    .ltorg
