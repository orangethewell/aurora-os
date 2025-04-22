use x86_64::instructions::port::Port;
use core::arch::asm;

/// Espera curta para I/O (barra 0x80)
#[inline(always)]
fn io_wait() {
    unsafe { asm!("out 0x80, al", in("al") 0u8); }
}

