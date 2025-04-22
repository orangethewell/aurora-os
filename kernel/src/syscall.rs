use core::arch::{asm, naked_asm};

const MSR_STAR: usize = 0xc0000081;
const MSR_LSTAR: usize = 0xc0000082;
const MSR_FMASK: usize = 0xc0000084;

#[naked]
extern "C" fn handle_syscall() {
    unsafe {
        naked_asm!(
            "mov rdr1, 0"
        );
    }
}

pub fn init() {
    let handler_addr = handle_syscall as *const () as u64;
    unsafe {
        asm!("mov ecx, 0xC0000080",
        "rdmsr",
        "or eax, 1",
        "wrmsr");
        
        asm!("xor rdx, rdx",
        "mov rax, 0x200",
        "wrmsr",
        in("rcx") MSR_FMASK);

        asm!("mov rdx, rax",
        "shr rdx, 32",
        "wrmsr",
        in("rax") handler_addr,
        in("rcx") MSR_LSTAR);

        asm!(
        "xor rax, rax",
        "mov rdx, 0x230008",
        "wrmsr",
        in("rcx") MSR_STAR);
    }
}