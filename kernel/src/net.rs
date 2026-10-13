use alloc::vec;
use alloc::vec::Vec;
use spin::Mutex;

use smoltcp::iface::{Config, Interface, SocketHandle, SocketSet};
use smoltcp::phy::{self, Device, DeviceCapabilities, Medium};
use smoltcp::socket::tcp::{self, Socket as TcpSocket};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpCidr, Ipv4Address, Ipv4Cidr};

use crate::e1000::{E1000, RX_BUFFER_SIZE};

/// A physical device adapter implementing smoltcp's `Device` trait for `E1000Device`.
pub struct SmoltcpDevice;

pub struct RxToken(Vec<u8>);
pub struct TxToken;

impl phy::RxToken for RxToken {
    fn consume<R, F>(self, f: F) -> R
    where
        F: FnOnce(&[u8]) -> R,
    {
        f(&self.0)
    }
}

impl phy::TxToken for TxToken {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0u8; len];
        let result = f(&mut buffer);
        if let Some(nic) = E1000.lock().as_mut() {
            let _ = nic.send_packet(&buffer);
        }
        result
    }
}

impl Device for SmoltcpDevice {
    type RxToken<'a> = RxToken where Self: 'a;
    type TxToken<'a> = TxToken where Self: 'a;

    fn receive(&mut self, _timestamp: Instant) -> Option<(Self::RxToken<'_>, Self::TxToken<'_>)> {
        let mut temp_buf = [0u8; RX_BUFFER_SIZE];
        let len = E1000.lock().as_mut()?.receive_packet(&mut temp_buf)?;
        Some((RxToken(temp_buf[..len].to_vec()), TxToken))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<Self::TxToken<'_>> {
        Some(TxToken)
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.medium = Medium::Ethernet;
        caps
    }
}

pub struct NetStack {
    pub iface: Interface,
    pub device: SmoltcpDevice,
    pub sockets: SocketSet<'static>,
    pub http_socket_handle: SocketHandle,
}

pub static NET: Mutex<Option<NetStack>> = Mutex::new(None);

/// Return the current time as a smoltcp `Instant`.
/// PIT timer in QEMU runs at ~18.2 Hz or ~55ms per tick by default.
pub fn now() -> Instant {
    let ticks = crate::interrupts::timer_ticks();
    // Approximate 55ms per tick
    Instant::from_millis((ticks * 55) as i64)
}

/// Initialize the network stack with IP 10.0.2.15/24 (standard QEMU SLIRP user network).
pub fn init() {
    let mac = match E1000.lock().as_ref() {
        Some(nic) => nic.mac_address(),
        None => {
            crate::println!("[net] Error: No network card found for smoltcp.");
            return;
        }
    };

    let hw_addr = HardwareAddress::Ethernet(EthernetAddress(mac));
    let mut device = SmoltcpDevice;

    let mut config = Config::new(hw_addr);
    config.random_seed = 0x12345678;

    let mut iface = Interface::new(config, &mut device, now());

    // QEMU user networking assigns 10.0.2.15 to the guest, 10.0.2.2 as gateway
    let ip_addr = IpCidr::Ipv4(Ipv4Cidr::new(Ipv4Address::new(10, 0, 2, 15), 24));
    iface.update_ip_addrs(|addrs| {
        let _ = addrs.push(ip_addr);
    });

    let default_gateway = Ipv4Address::new(10, 0, 2, 2);
    iface.routes_mut().add_default_ipv4_route(default_gateway).unwrap();

    // Create a TCP socket for our minimal web server (port 80)
    let rx_buffer = tcp::SocketBuffer::new(vec![0u8; 4096]);
    let tx_buffer = tcp::SocketBuffer::new(vec![0u8; 4096]);
    let mut http_socket = TcpSocket::new(rx_buffer, tx_buffer);
    http_socket.listen(80).expect("failed to listen on port 80");

    let mut sockets = SocketSet::new(vec![]);
    let http_socket_handle = sockets.add(http_socket);

    crate::println!("[net] Smoltcp initialized: IP 10.0.2.15/24, Gateway 10.0.2.2");
    crate::println!("[net] RustOS HTTP Web Server listening on port 80 (mapped to host:8080)");

    *NET.lock() = Some(NetStack {
        iface,
        device,
        sockets,
        http_socket_handle,
    });
}

/// Polls the network stack and handles HTTP requests.
pub fn poll() {
    let mut guard = NET.lock();
    let net = match guard.as_mut() {
        Some(n) => n,
        None => return,
    };

    let timestamp = now();
    net.iface.poll(timestamp, &mut net.device, &mut net.sockets);

    // Check HTTP socket activity
    let socket = net.sockets.get_mut::<TcpSocket>(net.http_socket_handle);
    if socket.can_recv() {
        let mut req_buf = [0u8; 1024];
        if let Ok(size) = socket.recv_slice(&mut req_buf) {
            if size > 0 {
                // Prepare a simple HTTP response
                let response = b"HTTP/1.1 200 OK\r\n\
Content-Type: text/html; charset=utf-8\r\n\
Content-Length: 99\r\n\
Connection: close\r\n\
\r\n\
<!DOCTYPE html><html><body><h1>Hello from Rust OS</h1>
This is a bare-metal minimal web server</body></html>";
                if socket.can_send() {
                    let _ = socket.send_slice(response);
                    socket.close();
                }
            }
        }
    } else if !socket.is_active() && !socket.is_listening() {
        // Re-listen when connection closes
        let _ = socket.listen(80);
    }
}

/// A background worker task that polls the network stack continuously.
pub fn net_task() -> ! {
    loop {
        poll();
        x86_64::instructions::hlt();
    }
}
