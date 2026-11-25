// VC4 control-list packets. Broadcom VideoCore IV 3D Architecture Reference
// Guide, "Control Records". One 64x64 non-MSAA tile covers the whole image.
.equ BUS, 0xc0000000                // Direct bus addresses for control lists and data
.equ CODE_BUS, 0x40000000           // QPU instruction fetch
.section .data.gpu,"aw",%progbits
.balign 4096
.global gpu_data, bin_list, bin_end, render_list, render_end, clear_color
.global fragment_code, fragment_end, shader_storage, fragment_color
gpu_data:
bin_list:
    .byte 112                       // Tile binning configuration
    .word tile_pool + BUS, 16384, tile_state + BUS
    .byte 1, 1, 4                   // Columns, rows, auto-initialize tile state
    .byte 6                         // Start tile binning
    .byte 102                       // Clip window
    .short 0, 0, 64, 64
    .byte 96, 3, 0x70, 0            // Both triangle windings, always pass depth test
    .byte 103                       // Viewport offset in sixteenths of a pixel
    .short 0, 0
    .byte 65                        // Pretransformed vertices, fragment shader only
    .word shader_record + BUS
    .byte 33, 4                     // Draw triangle list from consecutive vertices
    .word 3, 0
    .byte 4                         // Flush tile lists, increment BFC
bin_end:

.balign 64
render_list:
    .byte 114                       // Clear color, depth, mask and stencil
clear_color:
    .word 0, 0, 0
    .byte 0
    .byte 113                       // Linear RGBA8888 render target
    .word pixels + BUS
    .short 64, 64, 4
    .byte 115, 0, 0                 // Select tile, clear tile buffer
    .byte 28, 0, 0, 0, 0, 0, 0
    .byte 115, 0, 0
.global tile_branch
tile_branch:
    .byte 17                        // Call binner-generated tile sublist
    .word tile_pool + BUS
    .byte 25                        // Store tile, signal end of frame
render_end:

.balign 16
shader_record:
    .byte 1, 24, 0, 3               // Single threaded, vertex stride 24, RGB varyings
    .word shader_storage + CODE_BUS, 0, vertices + BUS
.balign 16
vertices:
    .short 32*16, 8*16
    .word 0x3f800000, 0x3f800000, 0x3e200000, 0x3f4e0000, 0x3f800000    // Z, reciprocal W, and three varying values
    .short 8*16, 56*16
    .word 0x3f800000, 0x3f800000, 0x3e200000, 0x3f4e0000, 0x3f800000
    .short 56*16, 56*16
    .word 0x3f800000, 0x3f800000, 0x3e200000, 0x3f4e0000, 0x3f800000

.balign 64
fragment_code:
    // Consume each of the three interpolated inputs before ending the thread.
    .word 0x158e7d80, 0x10020867    // mov r1, vary
    .word 0x158e7d80, 0x10020867    // mov r1, vary
    .word 0x158e7d80, 0x10020867    // mov r1, vary
fragment_color:
    .word 0xffffcd28, 0xe00208e7    // ldi r3, packed fragment color
    .word 0x009e7000, 0x400009e7    // Wait for scoreboard (after instruction two, HW-2796)
    .word 0x159e76c0, 0x30020ba7    // mov tlbc, r3 and end thread
    .word 0x009e7000, 0x100009e7    // First delay slot
    .word 0x009e7000, 0x500009e7    // Release scoreboard in the last delay slot
fragment_end:
.balign 64
.global gpu_data_end
gpu_data_end:

.section .bss.gpu,"aw",%nobits
.balign 4096
.global gpu_work, pixels, gpu_work_end
gpu_work:
tile_pool:  .space 16384
tile_state: .space 4096
pixels:     .space 16384
shader_storage: .space fragment_end - fragment_code
.balign 64
gpu_work_end:
