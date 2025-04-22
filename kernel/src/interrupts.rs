use core::arch::{asm, naked_asm};
use core::ptr::NonNull;
use alloc::boxed::Box;
use x2apic::lapic::{xapic_base, LocalApic, LocalApicBuilder};
use x2apic::ioapic::{IoApic, IrqFlags, IrqMode, RedirectionTableEntry};
use x86_64::structures::idt::{InterruptDescriptorTable, InterruptStackFrame};
use acpi::{AcpiTables, AcpiHandler, PhysicalMapping};
use x86_64::structures::idt::PageFaultErrorCode;
use x86_64::instructions::port::Port;
use lazy_static::lazy_static;
use x86_64::structures::paging::{FrameAllocator, Mapper, Size4KiB};
use x86_64::{PhysAddr, VirtAddr};
use crate::{gdt, process};
use crate::process::Context;

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

pub struct AcpiHandlerImpl {
    physical_memory_offset: VirtAddr,
}

impl AcpiHandlerImpl {
    pub fn new(physical_memory_offset: VirtAddr) -> Self {
        Self { physical_memory_offset }
    }
}

unsafe impl Send for AcpiHandlerImpl {}
unsafe impl Sync for AcpiHandlerImpl {}

impl Clone for AcpiHandlerImpl {
    fn clone(&self) -> Self {
        Self {
            physical_memory_offset: self.physical_memory_offset,
        }
    }
}

impl AcpiHandler for AcpiHandlerImpl {
    unsafe fn map_physical_region<T>(
        &self,
        physical_address: usize,
        size: usize,
    ) -> PhysicalMapping<Self, T> {
        let phys_addr = PhysAddr::new(physical_address as u64);
        let virt_addr = self.physical_memory_offset + phys_addr.as_u64();

        PhysicalMapping::new(
            physical_address,
            NonNull::new(virt_addr.as_mut_ptr()).expect("Failed to get virtual address"),
            size,
            size,
            self.clone(),
        )
    }

    fn unmap_physical_region<T>(_region: &PhysicalMapping<Self, T>) {
        // No unmapping necessary as we didn't create any new mappings
    }
}

static mut LAPIC: Option<NonNull<LocalApic>> = None;
static mut LAPIC_ID: u32 = 0;

static mut IOAPIC: Option<NonNull<IoApic>> = None;

pub unsafe fn init_lapic(lapic_phys: usize, physical_memory_offset: u64) {
    let lapic_virtual = lapic_phys as u64 + physical_memory_offset;

    let mut lapic = LocalApicBuilder::new()
        .timer_vector(InterruptIndex::Timer.as_usize())
        .error_vector(InterruptIndex::Error.as_usize())
        .spurious_vector(InterruptIndex::Spurious.as_usize())
        .set_xapic_base(lapic_virtual)
        .build()
        .expect("Failed to build LocalApic");

    lapic.enable();
    LAPIC_ID = lapic.id();

    let boxed = Box::leak(Box::new(lapic));
    LAPIC = Some(NonNull::from(boxed));
}

pub unsafe fn init_ioapic(ioapic_phys: usize, physical_memory_offset: u64, irq_offset: u8, lapic_id: u8) {
    let ioapic_virtual = ioapic_phys as u64 + physical_memory_offset;

    let mut ioapic = IoApic::new(ioapic_virtual);
    ioapic.init(irq_offset);

    // Configuração de exemplo para IRQ1 (teclado)
    let mut entry = RedirectionTableEntry::default();
    entry.set_vector(InterruptIndex::Keyboard.as_u8());
    entry.set_mode(IrqMode::Fixed);
    entry.set_flags(IrqFlags::LEVEL_TRIGGERED | IrqFlags::LOW_ACTIVE);
    entry.set_dest(lapic_id);

    ioapic.set_table_entry(1, entry);
    ioapic.enable_irq(1);

    let boxed = Box::leak(Box::new(ioapic));
    IOAPIC = Some(NonNull::from(boxed));
}

pub unsafe fn init_apic(
    rsdp: usize,
    physical_memory_offset: VirtAddr,
    mapper: &mut impl Mapper<Size4KiB>,
    frame_allocator: &mut impl FrameAllocator<Size4KiB>,
) {
    let handler = AcpiHandlerImpl::new(physical_memory_offset);
    let tables = AcpiTables::from_rsdp(handler, rsdp).expect("Failed to parse ACPI tables");
    let platform = tables.platform_info().expect("Failed to get platform info");

    match platform.interrupt_model {
        acpi::InterruptModel::Apic(apic) => {
            let ioapic_addr = apic.io_apics[0].address as usize;
            let lapic_addr = apic.local_apic_address as usize;

            init_lapic(lapic_addr, physical_memory_offset.as_u64());
            init_ioapic(ioapic_addr, physical_memory_offset.as_u64(), 32, get_current_lapic_id()); // irq_offset 32
        }
        _ => panic!("Unsupported APIC model"),
    }

    disable_pic();
}

pub fn get_current_lapic_id() -> u8 {
    unsafe { LAPIC_ID as u8 }
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
    Keyboard,
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

        unsafe {
            idt.page_fault.set_handler_fn(page_fault_handler)
                .set_stack_index(gdt::PAGE_FAULT_IST_INDEX);
        }
        serial_println!("IDT - Page Fault loaded");

        unsafe {
            idt[InterruptIndex::Timer.as_u8()]
                .set_handler_fn(timer_interrupt_handler)
                .set_stack_index(gdt::TIMER_INTERRUPT_INDEX);
        }
        serial_println!("IDT - APIC - Timer loaded");

        idt[InterruptIndex::Spurious.as_u8()]
            .set_handler_fn(spurious_interrupt_handler);
        serial_println!("IDT - APIC - Spurious loaded");

        idt[InterruptIndex::Error.as_u8()]
            .set_handler_fn(error_interrupt_handler);
        serial_println!("IDT - APIC - Error loaded");
        unsafe {
            idt[InterruptIndex::Keyboard.as_u8()]
                .set_handler_fn(keyboard_interrupt_handler)
                .set_stack_index(gdt::KEYBOARD_INTERRUPT_INDEX);
        }
        serial_println!("IDT - IOAPIC - Keyboard loaded");
        // Adicionando exceções
        idt.divide_error.set_handler_fn(divide_error_handler);
        serial_println!("IDT - Divide Error loaded");

        idt.debug.set_handler_fn(debug_handler);
        serial_println!("IDT - Debug loaded");

        idt.invalid_opcode.set_handler_fn(invalid_opcode_handler);
        serial_println!("IDT - Invalid Opcode loaded");

        unsafe {
            idt.general_protection_fault.set_handler_fn(general_protection_fault_handler)
                .set_stack_index(gdt::GENERAL_PROTECTION_FAULT_IST_INDEX);
        }
        serial_println!("IDT - General Protection Fault loaded");

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
    send_eoi();
}

extern "x86-interrupt" fn error_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    serial_println!("APIC Error Interrupt");
    send_eoi();
}

extern "x86-interrupt" fn breakpoint_handler(
    stack_frame: InterruptStackFrame)
{
    kprintln!("EXCEPTION: BREAKPOINT\n{:#?}", stack_frame);
}

extern "C" fn timer_handler(context_addr: usize) -> usize {
    let next_stack = process::schedule_next(context_addr);

    send_eoi();
    next_stack
}

#[naked]
pub extern "x86-interrupt" fn timer_interrupt_handler (
   _stack_frame: InterruptStackFrame) {
  unsafe {
    naked_asm!(
        // Disable interrupts
        "cli",
        // Push registers
        "push rax",
        "push rbx",
        "push rcx",
        "push rdx",
    
        "push rdi",
        "push rsi",
        "push rbp",
        "push r8",
    
        "push r9",
        "push r10",
        "push r11",
        "push r12",
    
        "push r13",
        "push r14",
        "push r15",
    
        // First argument in rdi with C calling convention
        "mov rdi, rsp",
        // Call the hander function
        "call {handler}",
        // New: stack pointer is in RAX
        "cmp rax, 0",
        "je 2f",        // if rax != 0 {
        "mov rsp, rax", //   rsp = rax;
        "2:",           // }
    
        // Pop scratch registers
        "pop r15",
        "pop r14",
        "pop r13",
    
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
    
        "pop r8",
        "pop rbp",
        "pop rsi",
        "pop rdi",
    
        "pop rdx",
        "pop rcx",
        "pop rbx",
        "pop rax",
        // Enable interrupts
        "sti",
        // Interrupt return
        "iretq",
        // Note: Getting the handler pointer here using `sym` operand, because
        // an `in` operand would clobber a register that we need to save, and we
        // can't have two asm blocks
        handler = sym timer_handler,
    );
  }
}

extern "x86-interrupt" fn keyboard_interrupt_handler(
    _stack_frame: InterruptStackFrame)
{
    use x86_64::instructions::port::Port;

    let mut port = Port::new(0x60); // Porta padrão do teclado
    let scancode: u8 = unsafe { port.read() };

    crate::task::keyboard::add_scancode(scancode);

    send_eoi(); // Sempre sinalize o fim
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

extern "x86-interrupt" fn divide_error_handler(
    stack_frame: InterruptStackFrame)
{
    kprintln!("EXCEPTION: DIVIDE BY ZERO\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn debug_handler(
    stack_frame: InterruptStackFrame)
{
    kprintln!("EXCEPTION: DEBUG\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn invalid_opcode_handler(
    stack_frame: InterruptStackFrame)
{
    kprintln!("EXCEPTION: INVALID OPCODE\n{:#?}", stack_frame);
}

extern "x86-interrupt" fn general_protection_fault_handler(
    stack_frame: InterruptStackFrame,
    error_code: u64,
)
{
    kprintln!("EXCEPTION: GENERAL PROTECTION FAULT");
    kprintln!("Error Code: {:#x}", error_code); 
    kprintln!("{:#?}", stack_frame); 
}
