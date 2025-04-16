use core::ptr::NonNull;

use alloc::boxed::Box;
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::instructions::port::Port;
use lazy_static::lazy_static;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder};
use crate::gdt;

pub const PIC_1_OFFSET: u8 = 32;
pub const PIC_2_OFFSET: u8 = PIC_1_OFFSET + 8;

pub fn disable_pic() {
    unsafe {
        let mut cmd_8259a = Port::<u8>::new(0x20);
        let mut data_8259a = Port::<u8>::new(0x21);
        let mut cmd_8259b = Port::<u8>::new(0xa0);
        let mut data_8259b = Port::<u8>::new(0xa1);

        let mut spin_port = Port::<u8>::new(0x80);
        let mut spin = || spin_port.write(0);

        // Inicia a reconfiguração (ICW1)
        cmd_8259a.write(0x11);
        cmd_8259b.write(0x11);
        spin();

        // Define os vetores de offset (ICW2)
        data_8259a.write(0xf8); // normalmente seria 0x20 (32)
        data_8259b.write(0xff); // normalmente seria 0x28 (40)
        spin();

        // Define a conexão mestre-escravo (ICW3)
        data_8259a.write(0b100); // escravo ligado à linha 2 do mestre
        data_8259b.write(0b10);  // escravo é a linha 2
        spin();

        // Modo 8086 (ICW4)
        data_8259a.write(0x1);
        data_8259b.write(0x1);
        spin();

        // Mascara todas as IRQs no final
        data_8259a.write(u8::MAX);
        data_8259b.write(u8::MAX);
    }
}

static mut LAPIC: Option<NonNull<LocalApic>> = None;

pub unsafe fn init_lapic(physical_memory_offset: u64) {
    let apic_physical_address = xapic_base();
    let apic_virtual_address = physical_memory_offset + apic_physical_address;

    let mut lapic = LocalApicBuilder::new()
        .timer_vector(InterruptIndex::Timer.as_usize())
        .error_vector(InterruptIndex::Error.as_usize())
        .spurious_vector(InterruptIndex::Spurious.as_usize())
        .set_xapic_base(apic_virtual_address)
        .build()
        .expect("Failed to build LocalApic");

    lapic.enable();

    // Aloca e guarda em ponteiro global
    let boxed = Box::leak(Box::new(lapic));
    LAPIC = Some(NonNull::from(boxed));
}

pub fn send_eoi() {
    unsafe {
        if let Some(mut apic_ptr) = LAPIC {
            let lapic = apic_ptr.as_mut();
            lapic.end_of_interrupt();
        }
    }
}

#[derive(Debug, Clone, Copy)]
#[repr(u8)]
pub enum InterruptIndex {
    Timer = PIC_1_OFFSET,
    Error,
    Spurious,

}

impl InterruptIndex {
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    pub fn as_usize(self) -> usize {
        usize::from(self.as_u8())
    }
}

lazy_static! {
    static ref IDT: InterruptDescriptorTable = {
        let mut idt = InterruptDescriptorTable::new();
        idt.breakpoint.set_handler_fn(breakpoint_handler);
        serial_println!("IDT - Breakpoint loaded");
        unsafe {
            idt.double_fault.set_handler_fn(double_fault_handler)
                .set_stack_index(gdt::DOUBLE_FAULT_IST_INDEX);
        }
        serial_println!("IDT - Double Fault loaded");
        idt.page_fault.set_handler_fn(page_fault_handler);
        serial_println!("IDT - Page Fault loaded");
        idt[InterruptIndex::Timer.as_u8()]
            .set_handler_fn(timer_interrupt_handler);
        serial_println!("IDT - APIC - Timer loaded");
        idt[InterruptIndex::Spurious.as_u8()]
            .set_handler_fn(spurious_interrupt_handler);
        serial_println!("IDT - APIC - Spurious loaded");
        idt[InterruptIndex::Error.as_u8()]
            .set_handler_fn(error_interrupt_handler);
        serial_println!("IDT - APIC - Error loaded");
        idt
    };
}

pub fn init_idt() {
    IDT.load();
}

extern "x86-interrupt" fn spurious_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    serial_println!("Spurious Interrupt");
    // Aqui você pode simplesmente ignorar ou registrar a interrupção
    send_eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    serial_println!("APIC Error Interrupt");
    // Trate o erro conforme necessário
    send_eoi();
}

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    kprintln!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn timer_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    kprint!(".");
    send_eoi();
    kprintln!("Following up next interrupt.");
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    serial_print!("k");
}

extern "x86-interrupt" fn double_fault_handler(
    stack_frame: InterruptStackFrame, _error_code: u64) -> !
{
    panic!("EXCEPTION: DOUBLE FAULT\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn page_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: PageFaultErrorCode,
) {
    use x86_64::registers::control::Cr2;

    kprintln!("EXCEPTION: PAGE FAULT");
    kprintln!("Accessed Address: {:?}", Cr2::read());
    kprintln!("Error Code: {:?}", error_code);
    kprintln!("{:#?}", stack_frame);
}