use super::queue::VIRTQ_DESC_F_WRITE;
use super::*;
use crate::drivers::{EthernetFrame, FrameDevice, MAX_ETHERNET_FRAME_LEN};

const ETHERNET_HEADER_LEN: usize = 14;

/// One receive buffer posted to the virtio-net RX queue.
pub struct RxBuffer {
    pub desc_idx: u16,
    pub virt: u64,
    pub phys: u64,
    pub len: u32,
    frame: u64,
}

struct NetTxBuffers {
    header_phys: u64,
    frame_phys: u64,
}

impl NetTxBuffers {
    fn new(eth_frame: &[u8]) -> Self {
        assert!(
            eth_frame.len() <= 4096,
            "single-frame TX buffer cannot hold {} bytes",
            eth_frame.len()
        );

        let header_frame =
            crate::frame_allocator::allocate().expect("out of frames for virtio-net TX header");
        let frame_frame =
            crate::frame_allocator::allocate().expect("out of frames for virtio-net TX frame");
        let offset = crate::memory::phys_offset();
        let header_phys = header_frame * 4096;
        let frame_phys = frame_frame * 4096;
        let header_virt = header_phys + offset;
        let frame_virt = frame_phys + offset;

        unsafe {
            core::ptr::write_bytes(header_virt as *mut u8, 0, 4096);
            core::ptr::write_bytes(frame_virt as *mut u8, 0, 4096);
            core::ptr::copy_nonoverlapping(
                eth_frame.as_ptr(),
                frame_virt as *mut u8,
                eth_frame.len(),
            );
        }

        Self {
            header_phys,
            frame_phys,
        }
    }
}

/// Driver state for the virtio-net device after PCI probing.
pub struct VirtioNet {
    bus: u8,
    device: u8,
    func: u8,
    common_virt: u64,
    notify_virt: u64,
    notify_off_multiplier: u32,
    device_config_virt: u64,
    mac: [u8; 6],
    rx_queue: Option<VirtQueue>,
    tx_queue: Option<VirtQueue>,
    rx_buffers: alloc::vec::Vec<RxBuffer>,
    msix: MsixCap,
}

impl FrameDevice for VirtioNet {
    fn mac_address(&self) -> [u8; 6] {
        self.mac
    }

    fn transmit(&mut self, eth_frame: &[u8]) -> bool {
        const ETHERNET_MIN_TX_BUFFER: usize = 64;
        assert!(
            eth_frame.len() >= ETHERNET_HEADER_LEN,
            "transmit needs at least an Ethernet header"
        );
        assert!(
            eth_frame.len() <= MAX_ETHERNET_FRAME_LEN,
            "Ethernet frame length {} exceeds {}",
            eth_frame.len(),
            MAX_ETHERNET_FRAME_LEN
        );

        let tx_len = core::cmp::max(eth_frame.len(), ETHERNET_MIN_TX_BUFFER);
        let buffers = NetTxBuffers::new(eth_frame);
        let (head, tail) = {
            let tx_queue = self
                .tx_queue
                .as_mut()
                .expect("virtio-net TX queue not configured");
            let (head, tail) = tx_queue.build_tx_chain(
                buffers.header_phys,
                VIRTIO_NET_HDR_LEN as u32,
                buffers.frame_phys,
                tx_len as u32,
            );
            tx_queue.submit_chain(head);
            (head, tail)
        };

        self.kick_queue(1);

        let interrupts_were_enabled = x86_64::instructions::interrupts::are_enabled();
        let completion = loop {
            x86_64::instructions::interrupts::disable();
            if let Some(completion) = self
                .tx_queue
                .as_mut()
                .expect("virtio-net TX queue not configured")
                .poll_completion()
            {
                break completion;
            }
            x86_64::instructions::interrupts::enable_and_hlt();
        };
        if interrupts_were_enabled {
            x86_64::instructions::interrupts::enable();
        }

        assert_eq!(
            completion.0, head as u32,
            "virtio-net TX completed unexpected descriptor"
        );

        let tx_queue = self.tx_queue.as_mut().unwrap();
        tx_queue.free_descriptor(head);
        tx_queue.free_descriptor(tail);

        println!(
            "[net] TX frame completion head={} len={} (descriptor bytes={})",
            completion.0,
            completion.1,
            VIRTIO_NET_HDR_LEN + tx_len
        );
        // The used-ring completion confirms the device consumed our buffers.
        true
    }

    fn receive(&mut self) -> Option<EthernetFrame> {
        let (desc_id, written_len) = {
            let rx_queue = self.rx_queue.as_mut()?;
            rx_queue.poll_completion()?
        };

        let desc_idx = desc_id as u16;
        let buffer = self
            .rx_buffers
            .iter()
            .find(|buffer| buffer.desc_idx == desc_idx)
            .expect("virtio-net RX completion used an unknown descriptor");
        let rx_len = core::cmp::min(written_len as usize, buffer.len as usize);

        let frame = if rx_len >= VIRTIO_NET_HDR_LEN + ETHERNET_HEADER_LEN {
            let eth_start = buffer.virt + VIRTIO_NET_HDR_LEN as u64;
            let eth_len = core::cmp::min(rx_len - VIRTIO_NET_HDR_LEN, MAX_ETHERNET_FRAME_LEN);
            let bytes = unsafe { core::slice::from_raw_parts(eth_start as *const u8, eth_len) };
            Some(EthernetFrame::from_bytes(bytes))
        } else {
            println!(
                "[net] RX dropped short virtio-net buffer desc={} len={}",
                desc_idx, rx_len
            );
            None
        };

        self.recycle_rx_buffer(desc_idx);
        frame
    }
}

impl VirtioNet {
    /// Find the modern virtio-net PCI device and map the common,
    /// notification, and device-config capability regions.
    pub fn probe() -> Option<Self> {
        let dev = pci::find_virtio_device(pci::VIRTIO_NET_NON_TRANSITIONAL)?;
        let caps = parse_virtio_capabilities(dev.bus, dev.device, dev.func);
        let msix = parse_msix_capability(dev.bus, dev.device, dev.func)?;

        let common_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_COMMON)?;
        let common_bar_base = pci::bar_base(dev.bus, dev.device, dev.func, common_cap.bar);
        let common_phys = common_bar_base + common_cap.offset as u64;
        let common_virt = crate::memory::map_mmio(common_phys, common_cap.length as u64);
        println!(
            "[virtio-net] common config: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X}",
            common_cap.bar, common_cap.offset, common_phys, common_virt
        );

        let notify_cap = caps.iter().find(|c| c.cfg_type == VIRTIO_CAP_NOTIFY)?;
        let notify_bar_base = pci::bar_base(dev.bus, dev.device, dev.func, notify_cap.bar);
        let notify_phys = notify_bar_base + notify_cap.offset as u64;
        let notify_virt = crate::memory::map_mmio(notify_phys, notify_cap.length as u64);
        println!(
            "[virtio-net] notifications: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X} (multiplier {})",
            notify_cap.bar,
            notify_cap.offset,
            notify_phys,
            notify_virt,
            notify_cap.notify_off_multiplier,
        );

        let device_config_cap = caps
            .iter()
            .find(|c| c.cfg_type == VIRTIO_CAP_DEVICE_CONFIG)?;
        let device_config_bar_base =
            pci::bar_base(dev.bus, dev.device, dev.func, device_config_cap.bar);
        let device_config_phys = device_config_bar_base + device_config_cap.offset as u64;
        let device_config_virt =
            crate::memory::map_mmio(device_config_phys, device_config_cap.length as u64);
        println!(
            "[virtio-net] device config: BAR{}+0x{:X} = phys 0x{:X} = virt 0x{:X}",
            device_config_cap.bar, device_config_cap.offset, device_config_phys, device_config_virt
        );

        Some(Self {
            bus: dev.bus,
            device: dev.device,
            func: dev.func,
            common_virt,
            notify_virt,
            notify_off_multiplier: notify_cap.notify_off_multiplier,
            device_config_virt,
            mac: [0; 6],
            rx_queue: None,
            tx_queue: None,
            rx_buffers: alloc::vec::Vec::new(),
            msix,
        })
    }

    fn read_common_u8(&self, offset: u8) -> u8 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u8) }
    }

    fn write_common_u8(&self, offset: u8, value: u8) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u8, value);
        }
    }

    fn read_common_u16(&self, offset: u8) -> u16 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u16) }
    }

    fn write_common_u16(&self, offset: u8, value: u16) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u16, value);
        }
    }

    fn read_common_u32(&self, offset: u8) -> u32 {
        unsafe { read_volatile((self.common_virt + offset as u64) as *const u32) }
    }

    fn write_common_u32(&self, offset: u8, value: u32) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u32, value);
        }
    }

    fn write_common_u64(&self, offset: u8, value: u64) {
        unsafe {
            write_volatile((self.common_virt + offset as u64) as *mut u64, value);
        }
    }

    pub fn read_status(&self) -> u8 {
        self.read_common_u8(COMMON_DEVICE_STATUS)
    }

    pub fn write_status(&self, value: u8) {
        self.write_common_u8(COMMON_DEVICE_STATUS, value);
    }

    pub fn reset_and_acknowledge(&self) {
        let before = self.read_status();
        println!(
            "[virtio-net] {:02x}:{:02x}.{} device_status before reset: 0x{:02X}",
            self.bus, self.device, self.func, before
        );

        self.write_status(0);
        let after_reset = self.read_status();
        println!(
            "[virtio-net] device_status after reset:  0x{:02X}",
            after_reset
        );
        assert_eq!(after_reset, 0, "virtio-net did not reset cleanly");

        self.write_status(STATUS_ACKNOWLEDGE);
        let after_ack = self.read_status();
        println!(
            "[virtio-net] device_status after ACK:    0x{:02X}",
            after_ack
        );

        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER);
        let after_driver = self.read_status();
        println!(
            "[virtio-net] device_status after DRIVER: 0x{:02X}",
            after_driver
        );
    }

    /// Negotiate the modern transport bit plus NET_F_MAC.
    pub fn negotiate_features(&self) {
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 0);
        let device_features_low = self.read_common_u32(COMMON_DEVICE_FEATURE);
        self.write_common_u32(COMMON_DEVICE_FEATURE_SELECT, 1);
        let device_features_high = self.read_common_u32(COMMON_DEVICE_FEATURE);

        println!(
            "[virtio-net] device features low:  0x{:08X}",
            device_features_low
        );
        println!(
            "[virtio-net] device features high: 0x{:08X}",
            device_features_high
        );

        assert!(
            device_features_low & VIRTIO_NET_F_MAC != 0,
            "virtio-net did not advertise VIRTIO_NET_F_MAC"
        );
        assert!(
            device_features_high & 0x1 != 0,
            "virtio-net did not advertise VIRTIO_F_VERSION_1"
        );

        let supported_low = VIRTIO_NET_F_MAC;
        let supported_high = 0x1;

        println!(
            "[virtio-net] driver features low:  0x{:08X} (VIRTIO_NET_F_MAC)",
            supported_low
        );
        println!(
            "[virtio-net] driver features high: 0x{:08X} (VIRTIO_F_VERSION_1)",
            supported_high
        );

        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 0);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_low);
        self.write_common_u32(COMMON_DRIVER_FEATURE_SELECT, 1);
        self.write_common_u32(COMMON_DRIVER_FEATURE, supported_high);

        self.write_status(STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK);
        let status = self.read_status();
        println!(
            "[virtio-net] device_status after FEATURES_OK: 0x{:02X}",
            status
        );
        assert!(
            status & STATUS_FEATURES_OK != 0,
            "virtio-net rejected our feature subset"
        );
    }

    /// Read and store the MAC address from device config offset 0.
    pub fn read_mac(&mut self) -> [u8; 6] {
        let mut mac = [0u8; 6];
        for (i, byte) in mac.iter_mut().enumerate() {
            *byte = unsafe { read_volatile((self.device_config_virt + i as u64) as *const u8) };
        }
        self.mac = mac;
        println!(
            "[virtio-net] MAC {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
            mac[0], mac[1], mac[2], mac[3], mac[4], mac[5]
        );
        mac
    }

    pub fn setup_queues(&mut self, queue_size: u16) {
        let rx_queue = VirtQueue::new(queue_size);
        self.configure_queue_at(0, &rx_queue, "RX");
        self.rx_queue = Some(rx_queue);

        let tx_queue = VirtQueue::new(queue_size);
        self.configure_queue_at(1, &tx_queue, "TX");
        self.tx_queue = Some(tx_queue);

        self.configure_queue_interrupts();
    }

    /// Point each queue at its own MSI-X table entry: RX queue 0 fires
    /// vector 0x30, TX queue 1 fires vector 0x31. MSI-X tables belong to
    /// individual PCI functions, so virtio-net starts at its own entry 0;
    /// virtio-blk's entry 0 lives in a different device's table.
    fn configure_queue_interrupts(&self) {
        assert!(
            self.msix.entries >= 2,
            "virtio-net MSI-X table has {} entries, need at least 2 (RX + TX)",
            self.msix.entries
        );
        for (entry_index, vector) in [
            (0u64, crate::arch::x86_64::interrupts::VIRTIO_NET_RX_VECTOR),
            (1u64, crate::arch::x86_64::interrupts::VIRTIO_NET_TX_VECTOR),
        ] {
            let entry = self.msix.table_virt + entry_index * 16;
            unsafe {
                write_volatile(entry as *mut u32, crate::arch::x86_64::apic::MSI_ADDRESS);
                write_volatile((entry + 4) as *mut u32, 0);
                write_volatile((entry + 8) as *mut u32, vector as u32);
                write_volatile((entry + 12) as *mut u32, 0);
            }
        }

        self.write_common_u16(COMMON_QUEUE_SELECT, 0);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 0);
        let rx_assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(rx_assigned, 0, "virtio-net RX queue rejected MSI-X entry 0");
        println!(
            "[virtio-net] MSI-X queue 0 (RX) -> table 0 -> vector 0x{:02x}",
            crate::arch::x86_64::interrupts::VIRTIO_NET_RX_VECTOR
        );

        self.write_common_u16(COMMON_QUEUE_SELECT, 1);
        self.write_common_u16(COMMON_QUEUE_MSIX_VECTOR, 1);
        let tx_assigned = self.read_common_u16(COMMON_QUEUE_MSIX_VECTOR);
        assert_eq!(tx_assigned, 1, "virtio-net TX queue rejected MSI-X entry 1");
        println!(
            "[virtio-net] MSI-X queue 1 (TX) -> table 1 -> vector 0x{:02x}",
            crate::arch::x86_64::interrupts::VIRTIO_NET_TX_VECTOR
        );
    }

    fn configure_queue_at(&self, queue_index: u16, queue: &VirtQueue, label: &str) {
        println!("[virtio-net] configuring queue {} ({})", queue_index, label);

        self.write_common_u16(COMMON_QUEUE_SELECT, queue_index);
        let device_max = self.read_common_u16(COMMON_QUEUE_SIZE);
        println!(
            "[virtio-net]   queue {} max size: {} (we want {})",
            queue_index,
            device_max,
            queue.queue_size()
        );
        assert!(
            queue.queue_size() <= device_max,
            "virtio-net queue {} max size {} < requested {}",
            queue_index,
            device_max,
            queue.queue_size()
        );

        self.write_common_u16(COMMON_QUEUE_SIZE, queue.queue_size());
        self.write_common_u64(COMMON_QUEUE_DESC, queue.desc_phys());
        self.write_common_u64(COMMON_QUEUE_AVAIL, queue.avail_phys());
        self.write_common_u64(COMMON_QUEUE_USED, queue.used_phys());
        self.write_common_u16(COMMON_QUEUE_ENABLE, 1);

        let enable_readback = self.read_common_u16(COMMON_QUEUE_ENABLE);
        println!(
            "[virtio-net]   queue {} enable readback: {}",
            queue_index, enable_readback
        );
        assert_eq!(
            enable_readback, 1,
            "virtio-net queue {} was rejected",
            queue_index
        );
        println!("[virtio-net] queue {} ({}) active", queue_index, label);
    }

    /// Allocate and publish the initial receive-buffer supply.
    pub fn post_initial_rx_buffers(&mut self) {
        let rx_queue = self
            .rx_queue
            .as_mut()
            .expect("virtio-net RX queue not configured");
        assert!(
            rx_queue.num_free() >= VIRTIO_NET_RX_BUFFER_COUNT,
            "not enough RX descriptors for initial supply"
        );

        for _ in 0..VIRTIO_NET_RX_BUFFER_COUNT {
            let frame =
                crate::frame_allocator::allocate().expect("out of frames for virtio-net RX buffer");
            let phys = frame * 4096;
            let virt = phys + crate::memory::phys_offset();

            unsafe {
                core::ptr::write_bytes(virt as *mut u8, 0, 4096);
            }

            let desc_idx = rx_queue.alloc_descriptor().expect("no free RX descriptors");
            rx_queue.write_descriptor(
                desc_idx,
                phys,
                VIRTIO_NET_RX_BUFFER_LEN,
                VIRTQ_DESC_F_WRITE,
                0,
            );
            rx_queue.submit_chain(desc_idx);

            self.rx_buffers.push(RxBuffer {
                desc_idx,
                virt,
                phys,
                len: VIRTIO_NET_RX_BUFFER_LEN,
                frame,
            });
        }

        let (avail_idx, used_idx, last_used_idx) = rx_queue.debug_state();
        if let Some(first) = self.rx_buffers.first() {
            println!(
                "[virtio-net] first RX buffer: desc={} frame={} phys=0x{:X} virt=0x{:X} len={}",
                first.desc_idx, first.frame, first.phys, first.virt, first.len
            );
        }
        println!(
            "[virtio-net] posted {} RX buffers (len={} bytes each, avail_idx={}, used_idx={}, last_used_idx={})",
            self.rx_buffers.len(),
            VIRTIO_NET_RX_BUFFER_LEN,
            avail_idx,
            used_idx,
            last_used_idx
        );
    }

    pub fn set_driver_ok(&self) {
        self.write_status(
            STATUS_ACKNOWLEDGE | STATUS_DRIVER | STATUS_FEATURES_OK | STATUS_DRIVER_OK,
        );
        let status = self.read_status();
        println!(
            "[virtio-net] device_status after DRIVER_OK: 0x{:02X}",
            status
        );
        assert!(
            status & STATUS_FAILED == 0,
            "virtio-net entered FAILED state"
        );
        assert!(
            status & STATUS_DEVICE_NEEDS_RESET == 0,
            "virtio-net requested reset"
        );
        assert!(
            status & STATUS_DRIVER_OK != 0,
            "virtio-net DRIVER_OK did not stick"
        );
        println!(
            "[virtio-net] device is live with RX stocked and TX idle (notify=0x{:X}, multiplier={})",
            self.notify_virt,
            self.notify_off_multiplier
        );
    }

    fn recycle_rx_buffer(&mut self, desc_idx: u16) {
        let buffer = self
            .rx_buffers
            .iter()
            .find(|buffer| buffer.desc_idx == desc_idx)
            .expect("cannot recycle unknown RX descriptor");

        unsafe {
            core::ptr::write_bytes(buffer.virt as *mut u8, 0, buffer.len as usize);
        }

        {
            let rx_queue = self
                .rx_queue
                .as_mut()
                .expect("virtio-net RX queue not configured");
            rx_queue.submit_chain(desc_idx);
            let (avail_idx, used_idx, last_used_idx) = rx_queue.debug_state();
            println!(
                "[net] RX buffer desc={} recycled (avail_idx={}, used_idx={}, last_used_idx={})",
                desc_idx, avail_idx, used_idx, last_used_idx
            );
        }

        self.kick_queue(0);
    }

    fn kick_queue(&self, queue_index: u16) {
        self.write_common_u16(COMMON_QUEUE_SELECT, queue_index);
        let queue_notify_off = self.read_common_u16(COMMON_QUEUE_NOTIFY_OFF) as u64;
        let off = queue_notify_off * (self.notify_off_multiplier as u64);
        let notify_addr = self.notify_virt + off;

        unsafe {
            write_volatile(notify_addr as *mut u16, queue_index);
        }
    }
}
