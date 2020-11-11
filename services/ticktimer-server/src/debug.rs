use core::fmt::{Error, Write};

use utralib::generated::*;

#[macro_export]
macro_rules! print
{
	($($args:tt)+) => ({
			use core::fmt::Write;
			let _ = write!(crate::debug::DEFAULT, $($args)+);
	});
}
#[macro_export]
macro_rules! println
{
	() => ({
		print!("\r\n")
	});
	($fmt:expr) => ({
		print!(concat!($fmt, "\r\n"))
	});
	($fmt:expr, $($args:tt)+) => ({
		print!(concat!($fmt, "\r\n"), $($args)+)
	});
}


fn handle_irq(irq_no: usize, arg: *mut usize) {
    print!("Handling IRQ {} (arg: {:08x}): ", irq_no, arg as usize);

    while let Some(c) = crate::debug::DEFAULT.getc() {
        print!("{}", c as char);
    }
    println!();
}

pub struct Uart {}

// this is a hack to bypass an explicit initialization/allocation step for the debug structure
pub static mut DEFAULT_UART_ADDR: *mut usize = 0x0000_0000 as *mut usize;

pub const DEFAULT: Uart = Uart {};

impl Uart {
    fn map_uart(&self) {
        /*
           Note: the memory address and interrupt specified here needs to map to a unique hardware
           UART resource. Modify in this function as necessary.
        */
        let uart = xous::syscall::map_memory(
            xous::MemoryAddress::new(utra::server1::HW_SERVER1_BASE),
            None,
            4096,
            xous::MemoryFlags::R | xous::MemoryFlags::W,
        )
        .expect("couldn't map debug uart");
        unsafe{ DEFAULT_UART_ADDR = uart.as_mut_ptr() as _; }
        println!("Mapped UART @ {:08x}", uart.addr.get());
        // core::mem::forget(uart);

        println!("Allocating IRQ...");
        xous::claim_interrupt(utra::server1::SERVER1_IRQ, handle_irq, core::ptr::null_mut::<usize>()).expect("unable to allocate IRQ");
        self.enable_rx();
    }

    pub fn putc(&self, c: u8) {
        if cfg!(feature = "debugprint") {
            if unsafe{DEFAULT_UART_ADDR} as usize == 0 {
                self.map_uart();
            }
            let mut uart_csr = CSR::new(unsafe{ DEFAULT_UART_ADDR as *mut u32});

            // Wait until TXFULL is `0`
            while uart_csr.r(utra::uart::TXFULL) != 0 {}
            uart_csr.wo(utra::uart::RXTX, c as u32);
        }
    }

    pub fn enable_rx(&self) {
        if cfg!(feature = "debugprint") {
            let mut uart_csr = CSR::new(unsafe{DEFAULT_UART_ADDR as *mut u32});
            uart_csr.wfo(utra::uart::EV_ENABLE_ENABLE, uart_csr.rf(utra::uart::EV_ENABLE_ENABLE) | 2 );
        }
    }

    pub fn getc(&self) -> Option<u8> {
        if cfg!(feature = "debugprint") {
            if unsafe{DEFAULT_UART_ADDR} as usize == 0 {
                self.map_uart();
            }
            let mut uart_csr = CSR::new(unsafe{DEFAULT_UART_ADDR as *mut u32});
            match uart_csr.rf(utra::uart::EV_PENDING_PENDING) & 2 {
                0 => None,
                ack => {
                    let c = Some(uart_csr.rf(utra::uart::RXTX_RXTX) as u8);
                    uart_csr.wo(utra::uart::EV_PENDING, ack);
                    c
                }
            }
        } else {
            None
        }
    }
}

impl Write for Uart {
    fn write_str(&mut self, s: &str) -> Result<(), Error> {
        for c in s.bytes() {
            self.putc(c);
        }
        Ok(())
    }
}
