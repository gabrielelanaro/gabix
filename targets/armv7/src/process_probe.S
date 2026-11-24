    .syntax unified
    .arm
    @ Shared probe data layout: see process.rs PROBE_* constants.
    .equ PROBE_ADDRESS_OFFSET, 4
    .equ PROBE_ACCESS_OFFSET, 8
    .equ PROBE_READ, 0
    .equ PROBE_EXECUTE, 2
    .equ SYS_WRITE, 1
    .equ SYS_GETPID, 39
    .equ SYS_EXIT, 60
    .section .rodata.process_probe,"a"
    .align 2
    .global process_probe_start
    .global process_probe_end
process_probe_start:
    @ Both copies execute at 0x41000000. Their private test data is at
    @ 0x41010000 and their private user stacks are near 0x41200000.
    mov r7, #SYS_GETPID                     @ getpid
    svc #0
    mov r4, r0
    movw r5, #0
    movt r5, #0x4101
    str r4, [r5]                    @ Same VA, different value in each process.
    push {r4, lr}
    bl .Lbusy                       @ Keep user LR live through timer switches.
    pop {r8, lr}
    cmp r8, r4
    bne .Lfailed
    ldr r8, [r5]
    cmp r8, r4
    bne .Lfailed

    ldr r1, [r5, #PROBE_ADDRESS_OFFSET]                @ Zero = normal exit; otherwise forbidden VA.
    cmp r1, #0
    beq .Lsuccess
    ldr r2, [r5, #PROBE_ACCESS_OFFSET]
    cmp r2, #PROBE_EXECUTE
    bxeq r1                         @ Execute-never user stack must fault.
    @ The write syscall must reject a pointer outside this process's user range.
    mov r0, #1
    mov r2, #4
    mov r7, #SYS_WRITE
    svc #0
    cmn r0, #1                      @ Expected u32::MAX / error.
    bne .Lfailed
    ldr r1, [r5, #PROBE_ADDRESS_OFFSET]
    ldr r2, [r5, #PROBE_ACCESS_OFFSET]                @ 0 = read attack, 1 = write attack.
    cmp r2, #PROBE_READ
    streq r2, [sp, #-4]!            @ Exercise process-private user stack again.
    ldreq r0, [r1]                  @ Must raise a user permission fault.
    strne r4, [r1]                  @ Must raise a user permission fault.
    b .Lfailed                      @ Reaching here means isolation failed.
.Lsuccess:
    mov r0, #0
    b .Lprocess_probe_exit
.Lfailed:
    mov r0, #1
.Lprocess_probe_exit:
    mov r7, #SYS_EXIT
    svc #0
    b .Lprocess_probe_exit
.Lbusy:
    movw r6, #0x2d00                @ 20,000,000 iterations, no yield syscall.
    movt r6, #0x0131
.Lloop:
    ldr r8, [r5]
    cmp r8, r4
    bne .Lfailed
    subs r6, r6, #1
    bne .Lloop
    bx lr
process_probe_end:
