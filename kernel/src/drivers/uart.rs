use alloc::sync::{self, Arc};
use heapless::Deque;
use lazy_static::lazy_static;

use crate::{
    arch::memlayout::UART_BASE_VADDR,
    devices::terminal::{InputReceiver, InputReciverList, TextScreen},
    proc::cpu::{intr_off_store, intr_restore},
    sync::{spin::Spinlock, wait::WaitQueue},
    utils::string::format_on_stack,
};

/// UART IRQ = 10 in QEMU virtual machine.
pub const UART_IRQ: u32 = 10;

// The UART control registers.

/// Receive holding register
pub const RHR: u8 = 0;
/// Transmit holding register
pub const THR: u8 = 0;

/// Interrupt enable register
pub const IER: u8 = 1;
pub const IER_RX_ENABLE: u8 = 1;
pub const IER_TX_ENABLE: u8 = 1 << 1;

/// FIFO control register
pub const FCR: u8 = 2;
pub const FCR_FIFO_ENABLE: u8 = 1;
pub const FCR_FIFO_CLEAR: u8 = 3 << 1;

/// Interrupt status register
pub const ISR: u8 = 2;

/// Line control register
pub const LCR: u8 = 3;
pub const LCR_EIGHT_BITS: u8 = 3;
/// Special mode to set baud rate
pub const LCR_BAUD_LATCH: u8 = 1 << 7;

/// line status register
pub const LSR: u8 = 5;
/// input is waiting to be read from RHR
pub const LSR_RX_READY: u8 = 1;
/// THR can accept another character to send
pub const LSR_TX_IDLE: u8 = 1 << 5;

pub fn uart_init() {
    // Disable interrupt.
    write_reg(IER, 0x00);

    // Special mode to set baud rate.
    write_reg(LCR, LCR_BAUD_LATCH);

    // LSB for baud rate of 38.4K.
    write_reg(0, 0x03);

    // MSB for baud rate of 38.4K.
    write_reg(1, 0x00);

    // Leave set-baud mode, and set word length to 8 bits, no parity.
    write_reg(LCR, LCR_EIGHT_BITS);

    // Reset and enable FIFOs.
    write_reg(FCR, FCR_FIFO_ENABLE | FCR_FIFO_CLEAR);

    // Enable only receive interrupts.
    write_reg(IER, /* IER_TX_ENABLE | */ IER_RX_ENABLE);
}

#[inline]
fn write_reg(reg: u8, val: u8) {
    unsafe {
        ((UART_BASE_VADDR + reg as usize) as *mut u8).write_volatile(val);
    };
}

#[inline]
fn read_reg(reg: u8) -> u8 {
    unsafe { ((UART_BASE_VADDR + reg as usize) as *mut u8).read_volatile() }
}

lazy_static! {
    pub static ref UART_RECEIVER_CLIENTS: InputReciverList = InputReciverList::new();
}

pub fn register_receiver(receiver: sync::Weak<dyn InputReceiver>) {
    UART_RECEIVER_CLIENTS.register(receiver);
}

pub fn uart_intr() {
    if read_reg(LSR) & LSR_TX_IDLE != 0 {
        let mut transmiter = UART_TRANSMITER.lock();
        transmiter.start_write();
    }

    let mut buf = [0_u8; 128];
    let mut len = 0;

    while read_reg(LSR) & LSR_RX_READY != 0 {
        let ch = read_reg(RHR);
        if len >= buf.len() {
            break;
        }
        buf[len] = ch;
        len += 1;
    }

    UART_RECEIVER_CLIENTS.receive_input(&buf[..len]);
}

lazy_static! {
    static ref UART_TRANSMITER: Spinlock<UartTransmiter> =
        Spinlock::new("uart_transmiter", UartTransmiter::new());
}

struct UartTransmiter {
    tx_ring_buf: Deque<u8, 1024>,
    wq: Arc<WaitQueue>,
}

impl UartTransmiter {
    #[must_use]
    pub fn new() -> Self {
        Self {
            tx_ring_buf: Deque::new(),
            wq: Arc::new(WaitQueue::with_name("uart_writer->wq")),
        }
    }

    fn start_write(&mut self) {
        while !self.tx_ring_buf.is_empty() {
            if read_reg(LSR) & LSR_TX_IDLE == 0 {
                // Handle this by uart_intr.
                return;
            }

            let ch = self.tx_ring_buf.pop_front().unwrap();
            write_reg(THR, ch);
        }

        // may be write_byte is waiting on the tx_ring_buf.
        self.wq.signal_all();
    }
}

pub fn uart_write_sync(bytes: &[u8]) {
    let s = intr_off_store();
    for byte in bytes {
        while read_reg(LSR) & LSR_TX_IDLE == 0 {
            core::hint::spin_loop();
        }
        write_reg(THR, *byte);
    }
    intr_restore(s);
}

pub fn uart_write(buf: &[u8]) {
    uart_write_sync(buf);

    // This is very slow.
    // TODO: remove or improve asyncronous writing.

    /*
    let mut writer = UART_TRANSMITER.lock_irq();
    let wq = writer.wq.clone();

    loop {
        let bytes_write = cmp::min(
            buf.len(),
            writer.tx_ring_buf.capacity() - writer.tx_ring_buf.len(),
        );

        for byte in &buf[..bytes_write] {
            unsafe { writer.tx_ring_buf.push_back_unchecked(*byte) };
        }

        writer.start_write();

        if bytes_write == buf.len() {
            return;
        }
        buf = &buf[bytes_write..];

        if writer.tx_ring_buf.is_full() {
            drop(writer);
            // Wait for uart_intr to handle this.
            wq.wait();
            writer = UART_TRANSMITER.lock_irq();
        }
    }*/
}

pub struct UartTextScreen;

impl TextScreen for UartTextScreen {
    fn write(&self, buf: &[u8]) -> crate::error::KResult<usize> {
        uart_write(buf);
        Ok(buf.len())
    }

    fn clear_screen(&self) -> crate::error::KResult<()> {
        uart_write_sync(b"\x1B[2J\x1B[H");
        Ok(())
    }

    fn insert_whitespace_at_cursor(&self) {
        uart_write_sync(b"\x1B[1@");
    }

    fn delete_char_before_cursor(&self) {
        uart_write_sync(b"\x1B[D\x1B[1P");
    }

    fn move_cursor_left(&self, n: usize) {
        if n != 0 {
            let str = format_on_stack!(128, "\x1B[{n}D");
            uart_write_sync(str.as_bytes());
        }
    }

    fn move_cursor_rigth(&self, n: usize) {
        if n != 0 {
            let str = format_on_stack!(128, "\x1B[{n}C");
            uart_write_sync(str.as_bytes());
        }
    }

    fn clear_to_end_of_line(&self) {
        uart_write_sync(b"\x1B[0K");
    }
}
