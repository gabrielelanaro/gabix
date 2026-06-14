//! Global Descriptor Table for our kernel.

use lazy_static::lazy_static;
use x86_64::instructions::segmentation::{CS, DS, ES, FS, GS, SS, Segment};
use x86_64::structures::gdt::{Descriptor, GlobalDescriptorTable, SegmentSelector};

lazy_static! {
    /// The kernel's GDT, paired with the selectors returned when we added
    /// each entry. We need the selectors later to reload the segment
    /// registers (CS, DS, SS).
    pub static ref GDT: (GlobalDescriptorTable, SegmentSelector, SegmentSelector) = {
        let mut gdt = GlobalDescriptorTable::new();
        // `kernel_code_segment()` and `kernel_data_segment()` are presets
        // in the x86_64 crate. They return a Descriptor with sensible
        // defaults: base 0, full limit, ring 0, present, 64-bit code/data.
        let code = gdt.append(Descriptor::kernel_code_segment());
        let data = gdt.append(Descriptor::kernel_data_segment());
        (gdt, code, data)
    };
}

/// Install our GDT and reload all segment registers to point at our entries.
pub fn init() {
    GDT.0.load();
    unsafe {
        // Data, stack, extra — plain `mov` works for these.
        DS::set_reg(GDT.2);
        SS::set_reg(GDT.2);
        ES::set_reg(GDT.2);
        // FS and GS — null selector (we don't use them yet).
        FS::set_reg(SegmentSelector::NULL);
        GS::set_reg(SegmentSelector::NULL);
        // CS is special: the crate internally uses a far-return trick
        // so the instruction pointer and code segment swap atomically.
        CS::set_reg(GDT.1);
    }
}
