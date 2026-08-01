use gabix_kernel::device::BlockDevice;
use gabix_kernel::fs::partition::{mbr_partition_start, OffsetBlockDevice};

struct Disk {
    sectors: [[u8; 512]; 4],
}

unsafe impl Send for Disk {}

impl BlockDevice for Disk {
    fn read_sector(&mut self, sector: u64) -> [u8; 512] {
        self.sectors[sector as usize]
    }
}

#[test]
fn test_mbr_finds_linux_partition() {
    let mut disk = Disk {
        sectors: [[0; 512]; 4],
    };
    disk.sectors[0][446 + 4] = 0x0c;
    disk.sectors[0][446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    disk.sectors[0][462 + 4] = 0x83;
    disk.sectors[0][462 + 8..462 + 12].copy_from_slice(&2u32.to_le_bytes());
    disk.sectors[0][510..512].copy_from_slice(&[0x55, 0xaa]);
    disk.sectors[2][..8].copy_from_slice(b"GABIXEXT");

    let start = mbr_partition_start(&mut disk, 0x83).unwrap();
    let mut partition = OffsetBlockDevice::new(&mut disk, start);

    assert_eq!(start, 2);
    assert_eq!(&partition.read_sector(0)[..8], b"GABIXEXT");
}
