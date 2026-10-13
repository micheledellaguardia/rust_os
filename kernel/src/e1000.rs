//! Intel 82540EM (e1000) Gigabit Ethernet Controller Driver.
//!
//! Provides basic transmit and receive functionality using DMA descriptor rings.

use alloc::boxed::Box;
use alloc::vec::Vec;
use spin::Mutex;
use x86_64::VirtAddr;

use crate::pci::PciDevice;

// e1000 Register Offsets
const REG_CTRL: usize = 0x0000;      // Device Control
const REG_STATUS: usize = 0x0008;    // Device Status
const REG_EERD: usize = 0x0014;      // EEPROM Read
const REG_ICR: usize = 0x00C0;       // Interrupt Cause Read
const REG_IMS: usize = 0x00D0;       // Interrupt Mask Set
const REG_RCTL: usize = 0x0100;      // Receive Control
const REG_RDBAL: usize = 0x2800;     // RX Descriptor Base Address Low
const REG_RDBAH: usize = 0x2804;     // RX Descriptor Base Address High
const REG_RDLEN: usize = 0x2808;     // RX Descriptor Length
const REG_RDH: usize = 0x2810;       // RX Descriptor Head
const REG_RDT: usize = 0x2818;       // RX Descriptor Tail
const REG_TCTL: usize = 0x0400;      // Transmit Control
const REG_TDBAL: usize = 0x3800;     // TX Descriptor Base Address Low
const REG_TDBAH: usize = 0x3804;     // TX Descriptor Base Address High
const REG_TDLEN: usize = 0x3808;     // TX Descriptor Length
const REG_TDH: usize = 0x3810;       // TX Descriptor Head
const REG_TDT: usize = 0x3818;       // TX Descriptor Tail
const REG_MTA: usize = 0x5200;       // Multicast Table Array (0x5200 - 0x53FC)

// Control Register Bits
const CTRL_SLU: u32 = 1 << 6;        // Set Link Up
const CTRL_RST: u32 = 1 << 26;       // Device Reset

// RCTL (Receive Control) Bits
const RCTL_EN: u32 = 1 << 1;         // Receiver Enable
const RCTL_SBP: u32 = 1 << 2;        // Store Bad Packets
const RCTL_UPE: u32 = 1 << 3;        // Unicast Promiscuous Enabled
const RCTL_MPE: u32 = 1 << 4;        // Multicast Promiscuous Enabled
const RCTL_LBM_NONE: u32 = 0 << 6;   // No loopback
const RCTL_BAM: u32 = 1 << 15;       // Broadcast Accept Mode
const RCTL_BSIZE_2048: u32 = 0 << 16;// Buffer Size 2048 Bytes
const RCTL_SECRC: u32 = 1 << 26;     // Strip Ethernet CRC

// TCTL (Transmit Control) Bits
const TCTL_EN: u32 = 1 << 1;         // Transmitter Enable
const TCTL_PSP: u32 = 1 << 3;        // Pad Short Packets
const TCTL_CT_SHIFT: u32 = 4;        // Collision Threshold
const TCTL_COLD_SHIFT: u32 = 12;     // Collision Distance

// Descriptors count (must be multiple of 8 or 128 bytes)
const NUM_RX_DESCRIPTORS: usize = 32;
const NUM_TX_DESCRIPTORS: usize = 32;
pub const RX_BUFFER_SIZE: usize = 2048;

/// Legacy Receive Descriptor (16 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RxDescriptor {
    buffer_addr: u64,
    length: u16,
    checksum: u16,
    status: u8,
    errors: u8,
    special: u16,
}

/// Legacy Transmit Descriptor (16 bytes)
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TxDescriptor {
    buffer_addr: u64,
    length: u16,
    cso: u8,
    cmd: u8,
    status: u8,
    css: u8,
    special: u16,
}

pub struct E1000Device {
    mmio_base: VirtAddr,
    phys_offset: VirtAddr,
    mac_address: [u8; 6],
    rx_descriptors: Box<[RxDescriptor; NUM_RX_DESCRIPTORS]>,
    rx_buffers: Vec<Box<[u8; RX_BUFFER_SIZE]>>,
    rx_cur: usize,
    tx_descriptors: Box<[TxDescriptor; NUM_TX_DESCRIPTORS]>,
    tx_buffers: Vec<Box<[u8; RX_BUFFER_SIZE]>>,
    tx_cur: usize,
    pub tx_count: u64,
    pub rx_count: u64,
}

pub static E1000: Mutex<Option<E1000Device>> = Mutex::new(None);

impl E1000Device {
    /// Read a 32-bit register from MMIO space.
    fn read_reg(&self, reg: usize) -> u32 {
        unsafe {
            let ptr = (self.mmio_base + reg as u64).as_ptr::<u32>();
            core::ptr::read_volatile(ptr)
        }
    }

    /// Write a 32-bit register to MMIO space.
    fn write_reg(&self, reg: usize, value: u32) {
        unsafe {
            let ptr = (self.mmio_base + reg as u64).as_mut_ptr::<u32>();
            core::ptr::write_volatile(ptr, value);
        }
    }

    /// Reads a 16-bit word from the EEPROM.
    fn read_eeprom(&self, addr: u8) -> u16 {
        // EERD: Bit 0 = Start, Bits 8..15 = Address
        self.write_reg(REG_EERD, 1 | ((addr as u32) << 8));
        loop {
            let val = self.read_reg(REG_EERD);
            if (val & (1 << 4)) != 0 {
                // Done bit set
                return ((val >> 16) & 0xFFFF) as u16;
            }
        }
    }

    /// Reads the hardware MAC address.
    fn read_mac(&self) -> [u8; 6] {
        let w0 = self.read_eeprom(0);
        let w1 = self.read_eeprom(1);
        let w2 = self.read_eeprom(2);
        [
            (w0 & 0xFF) as u8,
            ((w0 >> 8) & 0xFF) as u8,
            (w1 & 0xFF) as u8,
            ((w1 >> 8) & 0xFF) as u8,
            (w2 & 0xFF) as u8,
            ((w2 >> 8) & 0xFF) as u8,
        ]
    }

    /// Translates any virtual address (heap, stack, or offset-mapped) into a physical address.
    fn virt_to_phys(&self, virt: u64) -> u64 {
        // Our kernel heap starts at 0x4444_4444_0000. Any heap address must be translated via the page table!
        if virt >= crate::memory::HEAP_START && virt < crate::memory::HEAP_START + (crate::memory::HEAP_SIZE as u64) {
            crate::memory::translate_addr(VirtAddr::new(virt), self.phys_offset)
                .expect("failed to translate DMA heap buffer virtual address to physical")
                .as_u64()
        } else if virt >= self.phys_offset.as_u64() {
            // Direct physical memory offset mapping
            virt - self.phys_offset.as_u64()
        } else {
            crate::memory::translate_addr(VirtAddr::new(virt), self.phys_offset)
                .expect("failed to translate DMA buffer virtual address to physical")
                .as_u64()
        }
    }

    pub fn mac_address(&self) -> [u8; 6] {
        self.mac_address
    }

    /// Transmit an Ethernet frame.
    pub fn send_packet(&mut self, data: &[u8]) -> Result<(), &'static str> {
        if data.len() > RX_BUFFER_SIZE {
            return Err("packet too large");
        }

        let desc = &mut self.tx_descriptors[self.tx_cur];
        // Bit 0: DD (Descriptor Done). The descriptor is free for reuse once the
        // NIC has set DD; while DD is clear the previous frame is still in flight.
        if (desc.status & 1) == 0 {
            return Err("tx queue full");
        }

        // Copy frame data into the dedicated DMA buffer
        self.tx_buffers[self.tx_cur][..data.len()].copy_from_slice(data);
        desc.length = data.len() as u16;
        // CMD: Bit 0 = EOP (End Of Packet), Bit 1 = IFCS (Insert FCS/CRC), Bit 3 = RS (Report Status)
        desc.cmd = (1 << 0) | (1 << 1) | (1 << 3);
        desc.status = 0;

        let prev = self.tx_cur;
        self.tx_cur = (self.tx_cur + 1) % NUM_TX_DESCRIPTORS;
        self.write_reg(REG_TDT, self.tx_cur as u32);

        // Wait briefly for transmission to finish
        let mut attempts = 0;
        while (self.tx_descriptors[prev].status & 1) == 0 {
            attempts += 1;
            if attempts > 100_000 {
                return Err("tx timeout");
            }
            core::hint::spin_loop();
        }

        self.tx_count += 1;
        Ok(())
    }

    /// Poll for an incoming Ethernet frame. Returns the packet length if received.
    pub fn receive_packet<'a>(&mut self, out_buf: &'a mut [u8]) -> Option<usize> {
        let desc = &mut self.rx_descriptors[self.rx_cur];
        // Bit 0: DD (Descriptor Done), Bit 1: EOP (End Of Packet)
        if (desc.status & 1) == 0 {
            return None; // No packet ready
        }

        let len = desc.length as usize;
        let copy_len = len.min(out_buf.len());
        out_buf[..copy_len].copy_from_slice(&self.rx_buffers[self.rx_cur][..copy_len]);

        // Reset descriptor status for reuse
        desc.status = 0;
        let old_cur = self.rx_cur;
        self.rx_cur = (self.rx_cur + 1) % NUM_RX_DESCRIPTORS;
        self.write_reg(REG_RDT, old_cur as u32);

        self.rx_count += 1;
        Some(copy_len)
    }
}

/// Initializes the Intel e1000 network card.
pub fn init(pci_dev: &PciDevice, mmio_base: VirtAddr, phys_offset: VirtAddr) {
    pci_dev.enable_bus_mastering();

    // Prepare RX buffers and descriptors
    let rx_descriptors = Box::new([RxDescriptor {
        buffer_addr: 0,
        length: 0,
        checksum: 0,
        status: 0,
        errors: 0,
        special: 0,
    }; NUM_RX_DESCRIPTORS]);

    let mut rx_buffers = Vec::with_capacity(NUM_RX_DESCRIPTORS);
    for _ in 0..NUM_RX_DESCRIPTORS {
        rx_buffers.push(Box::new([0u8; RX_BUFFER_SIZE]));
    }

    // Prepare TX buffers and descriptors
    let tx_descriptors = Box::new([TxDescriptor {
        buffer_addr: 0,
        length: 0,
        cso: 0,
        cmd: 0,
        status: 1, // Start with DD bit set so descriptor is free
        css: 0,
        special: 0,
    }; NUM_TX_DESCRIPTORS]);

    let mut tx_buffers = Vec::with_capacity(NUM_TX_DESCRIPTORS);
    for _ in 0..NUM_TX_DESCRIPTORS {
        tx_buffers.push(Box::new([0u8; RX_BUFFER_SIZE]));
    }

    let mut dev = E1000Device {
        mmio_base,
        phys_offset,
        mac_address: [0; 6],
        rx_descriptors,
        rx_buffers,
        rx_cur: 0,
        tx_descriptors,
        tx_buffers,
        tx_cur: 0,
        tx_count: 0,
        rx_count: 0,
    };

    // Prepare RX buffers and descriptors using true physical addresses
    for i in 0..NUM_RX_DESCRIPTORS {
        let buf_phys = dev.virt_to_phys(dev.rx_buffers[i].as_ptr() as u64);
        dev.rx_descriptors[i].buffer_addr = buf_phys;
        dev.rx_descriptors[i].status = 0;
    }

    // Prepare TX buffers and descriptors using true physical addresses
    for i in 0..NUM_TX_DESCRIPTORS {
        let buf_phys = dev.virt_to_phys(dev.tx_buffers[i].as_ptr() as u64);
        dev.tx_descriptors[i].buffer_addr = buf_phys;
        dev.tx_descriptors[i].status = 1;
    }

    // Read MAC address from EEPROM
    dev.mac_address = dev.read_mac();
    crate::println!(
        "[e1000] MAC Address: {:02x}:{:02x}:{:02x}:{:02x}:{:02x}:{:02x}",
        dev.mac_address[0],
        dev.mac_address[1],
        dev.mac_address[2],
        dev.mac_address[3],
        dev.mac_address[4],
        dev.mac_address[5]
    );

    // Link setup: Set Link Up (SLU)
    let ctrl = dev.read_reg(REG_CTRL);
    dev.write_reg(REG_CTRL, ctrl | CTRL_SLU);

    // Clear Multicast Table Array
    for i in 0..128 {
        dev.write_reg(REG_MTA + i * 4, 0);
    }

    // Disable interrupts for simple polled/event driven mode for now
    dev.write_reg(REG_IMS, 0);
    let _ = dev.read_reg(REG_ICR);

    // Setup Receive Descriptor Ring
    let rx_ring_phys = dev.virt_to_phys(dev.rx_descriptors.as_ptr() as u64);
    dev.write_reg(REG_RDBAL, (rx_ring_phys & 0xFFFF_FFFF) as u32);
    dev.write_reg(REG_RDBAH, (rx_ring_phys >> 32) as u32);
    dev.write_reg(REG_RDLEN, (NUM_RX_DESCRIPTORS * core::mem::size_of::<RxDescriptor>()) as u32);
    dev.write_reg(REG_RDH, 0);
    dev.write_reg(REG_RDT, (NUM_RX_DESCRIPTORS - 1) as u32);

    // Enable Receiver: EN + BAM + UPE + MPE + SECRC + BSIZE_2048
    dev.write_reg(
        REG_RCTL,
        RCTL_EN | RCTL_BAM | RCTL_UPE | RCTL_MPE | RCTL_SECRC | RCTL_BSIZE_2048,
    );

    // Setup Transmit Descriptor Ring
    let tx_ring_phys = dev.virt_to_phys(dev.tx_descriptors.as_ptr() as u64);
    dev.write_reg(REG_TDBAL, (tx_ring_phys & 0xFFFF_FFFF) as u32);
    dev.write_reg(REG_TDBAH, (tx_ring_phys >> 32) as u32);
    dev.write_reg(REG_TDLEN, (NUM_TX_DESCRIPTORS * core::mem::size_of::<TxDescriptor>()) as u32);
    dev.write_reg(REG_TDH, 0);
    dev.write_reg(REG_TDT, 0);

    // Enable Transmitter: EN + PSP + CT(15) + COLD(64)
    dev.write_reg(
        REG_TCTL,
        TCTL_EN | TCTL_PSP | (15 << TCTL_CT_SHIFT) | (64 << TCTL_COLD_SHIFT),
    );

    crate::println!("[e1000] Receiver and transmitter enabled successfully.");
    *E1000.lock() = Some(dev);
}
