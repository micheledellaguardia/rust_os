//! PCI Configuration Space access and bus enumeration.
//!
//! Standard x86 PCI Configuration Mechanism #1 uses two 32-bit I/O ports:
//! - 0xCF8: PCI Configuration Address Port (`CONFIG_ADDRESS`)
//! - 0xCFC: PCI Configuration Data Port (`CONFIG_DATA`)

use x86_64::instructions::port::Port;

const CONFIG_ADDRESS_PORT: u16 = 0xCF8;
const CONFIG_DATA_PORT: u16 = 0xCFC;

/// Represents a discovered PCI device.
#[derive(Debug, Clone, Copy)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_code: u8,
    pub subclass: u8,
    pub header_type: u8,
}

impl PciDevice {
    /// Read a 32-bit value from this device's configuration space.
    pub fn read_config_u32(&self, offset: u8) -> u32 {
        read_config_u32(self.bus, self.device, self.function, offset)
    }

    /// Write a 32-bit value to this device's configuration space.
    pub fn write_config_u32(&self, offset: u8, value: u32) {
        write_config_u32(self.bus, self.device, self.function, offset, value);
    }

    /// Read Base Address Register (BAR 0..5).
    pub fn read_bar(&self, bar_index: u8) -> u32 {
        assert!(bar_index < 6, "PCI BAR index must be 0..5");
        self.read_config_u32(0x10 + bar_index * 4)
    }

    /// Enable Bus Mastering and Memory Space access for this PCI device.
    /// This is required for network cards performing DMA / MMIO.
    pub fn enable_bus_mastering(&self) {
        let command_and_status = self.read_config_u32(0x04);
        let mut command = (command_and_status & 0xFFFF) as u16;
        // Bit 0: I/O Space, Bit 1: Memory Space, Bit 2: Bus Master
        command |= (1 << 1) | (1 << 2);
        let updated = (command_and_status & 0xFFFF_0000) | (command as u32);
        self.write_config_u32(0x04, updated);
    }
}

/// Computes the CONFIG_ADDRESS value according to the PCI specification.
fn pci_config_address(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let bus = bus as u32;
    let device = device as u32;
    let function = function as u32;
    let offset = (offset & 0xFC) as u32; // 32-bit aligned offset

    // Bit 31: Enable bit
    (1 << 31) | (bus << 16) | (device << 11) | (function << 8) | offset
}

/// Reads a 32-bit word from PCI configuration space.
pub fn read_config_u32(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address = pci_config_address(bus, device, function, offset);
    unsafe {
        let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(CONFIG_DATA_PORT);
        addr_port.write(address);
        data_port.read()
    }
}

/// Writes a 32-bit word to PCI configuration space.
pub fn write_config_u32(bus: u8, device: u8, function: u8, offset: u8, value: u32) {
    let address = pci_config_address(bus, device, function, offset);
    unsafe {
        let mut addr_port = Port::<u32>::new(CONFIG_ADDRESS_PORT);
        let mut data_port = Port::<u32>::new(CONFIG_DATA_PORT);
        addr_port.write(address);
        data_port.write(value);
    }
}

/// Reads a 16-bit word from PCI configuration space.
pub fn read_config_u16(bus: u8, device: u8, function: u8, offset: u8) -> u16 {
    let value = read_config_u32(bus, device, function, offset);
    if (offset & 2) == 0 {
        (value & 0xFFFF) as u16
    } else {
        ((value >> 16) & 0xFFFF) as u16
    }
}

/// Scans the PCI bus and calls `f` for each discovered device.
pub fn scan_bus<F>(mut f: F)
where
    F: FnMut(PciDevice),
{
    for bus in 0..=255 {
        for device in 0..32 {
            let vendor_id = read_config_u16(bus, device, 0, 0x00);
            if vendor_id == 0xFFFF {
                // Device does not exist
                continue;
            }

            let header_type_reg = read_config_u16(bus, device, 0, 0x0E);
            let header_type = (header_type_reg & 0xFF) as u8;
            let num_functions = if (header_type & 0x80) != 0 { 8 } else { 1 };

            for function in 0..num_functions {
                let vendor_id = read_config_u16(bus, device, function, 0x00);
                if vendor_id == 0xFFFF {
                    continue;
                }
                let device_id = read_config_u16(bus, device, function, 0x02);
                let class_subclass = read_config_u16(bus, device, function, 0x0A);
                let class_code = ((class_subclass >> 8) & 0xFF) as u8;
                let subclass = (class_subclass & 0xFF) as u8;

                let pci_dev = PciDevice {
                    bus,
                    device,
                    function,
                    vendor_id,
                    device_id,
                    class_code,
                    subclass,
                    header_type: header_type & 0x7F,
                };
                f(pci_dev);
            }
        }
    }
}

/// Finds a specific PCI device by Vendor ID and Device ID (e.g. Intel 82540EM = 0x8086:0x100E).
pub fn find_device(vendor_id: u16, device_id: u16) -> Option<PciDevice> {
    let mut found = None;
    scan_bus(|dev| {
        if dev.vendor_id == vendor_id && dev.device_id == device_id && found.is_none() {
            found = Some(dev);
        }
    });
    found
}
