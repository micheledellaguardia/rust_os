//! PS/2 keyboard handling: decodes scancodes and stores printable keys in a
//! small lock-free (mutex-protected) ring buffer for a future shell.

use pc_keyboard::{layouts, DecodedKey, HandleControl, Keyboard, ScancodeSet1};
use spin::{Mutex, Once};
use x86_64::instructions::port::Port;

static KEYBOARD: Once<Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>>> = Once::new();

fn keyboard() -> &'static Mutex<Keyboard<layouts::Us104Key, ScancodeSet1>> {
    KEYBOARD.call_once(|| {
        Mutex::new(Keyboard::new(
            ScancodeSet1::new(),
            layouts::Us104Key,
            HandleControl::Ignore,
        ))
    })
}

const BUFFER_SIZE: usize = 256;

struct InputBuffer {
    data: [u8; BUFFER_SIZE],
    head: usize,
    tail: usize,
}

impl InputBuffer {
    const fn new() -> Self {
        InputBuffer {
            data: [0; BUFFER_SIZE],
            head: 0,
            tail: 0,
        }
    }

    fn push(&mut self, byte: u8) {
        let next = (self.head + 1) % BUFFER_SIZE;
        if next != self.tail {
            self.data[self.head] = byte;
            self.head = next;
        }
    }

    fn pop(&mut self) -> Option<u8> {
        if self.head == self.tail {
            None
        } else {
            let byte = self.data[self.tail];
            self.tail = (self.tail + 1) % BUFFER_SIZE;
            Some(byte)
        }
    }
}

static INPUT: Mutex<InputBuffer> = Mutex::new(InputBuffer::new());

/// Pop the next buffered key press, if any.
pub fn read_char() -> Option<u8> {
    INPUT.lock().pop()
}

/// Called from the keyboard IRQ handler.
pub fn handle_interrupt() {
    let mut keyboard = keyboard().lock();
    let mut port = Port::new(0x60);
    let scancode: u8 = unsafe { port.read() };

    if let Ok(Some(event)) = keyboard.add_byte(scancode) {
        if let Some(key) = keyboard.process_keyevent(event) {
            match key {
                DecodedKey::Unicode(character) => {
                    // Stay allocation-free: the keyboard IRQ may fire before the
                    // heap is initialized. We do not print here either, because
                    // the framebuffer lock could be held by a preempted task.
                    // The shell echoes the characters it reads.
                    if (character as u32) < 0x80 {
                        INPUT.lock().push(character as u8);
                    }
                }
                DecodedKey::RawKey(_) => {}
            }
        }
    }
}
