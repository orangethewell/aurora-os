use spin::Mutex;
use x86_64::VirtAddr;
use x86_64::structures::tss::TaskStateSegment;
use x86_64::structures::gdt::{GlobalDescriptorTable, Descriptor, SegmentSelector};
use x86_64::instructions::segmentation::Segment;
use lazy_static::lazy_static;

pub const DOUBLE_FAULT_IST_INDEX: u16 = 0;
pub const PAGE_FAULT_IST_INDEX: u16 = 0;
pub const GENERAL_PROTECTION_FAULT_IST_INDEX: u16 = 0;
pub const TIMER_INTERRUPT_INDEX: u16 = 1;
pub const KEYBOARD_INTERRUPT_INDEX: u16 = 0;

lazy_static! {
    static ref TSS: Mutex<TaskStateSegment> = {
        let mut tss = TaskStateSegment::new();
        tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize] = {
            const STACK_SIZE: usize = 4096 * 5;
            static mut STACK: [u8; STACK_SIZE] = [0; STACK_SIZE];

            let stack_start = VirtAddr::from_ptr(&raw const STACK);
            let stack_end = stack_start + STACK_SIZE.try_into().unwrap();
            stack_end
        };

        tss.interrupt_stack_table[TIMER_INTERRUPT_INDEX as usize] =
            tss.interrupt_stack_table[DOUBLE_FAULT_IST_INDEX as usize];

        Mutex::new(tss)
    };
}

unsafe fn tss_reference() -> &'static TaskStateSegment {
    let tss_ptr = &*TSS.lock() as *const TaskStateSegment;
    & *tss_ptr
}

pub fn set_interrupt_stack_table(index: usize, stack_end: VirtAddr) {
    TSS.lock().interrupt_stack_table[index] = stack_end;
}

lazy_static! {
    static ref GDT: (GlobalDescriptorTable, Selectors) = {
        let mut gdt = GlobalDescriptorTable::new();
        let code_selector = gdt.append(Descriptor::kernel_code_segment());
        let data_selector = gdt.append(Descriptor::kernel_data_segment());
        let tss_selector = gdt.append(Descriptor::tss_segment(unsafe {tss_reference()}));
        let user_code_selector = gdt.append(Descriptor::user_code_segment());
        let user_data_selector = gdt.append(Descriptor::user_data_segment());
        (gdt, Selectors { code_selector, data_selector, tss_selector, user_code_selector, user_data_selector })
    };
}


struct Selectors {
    code_selector: SegmentSelector,
    data_selector: SegmentSelector,
    tss_selector: SegmentSelector,
    user_code_selector: SegmentSelector,
    user_data_selector: SegmentSelector
}

pub fn init() {
    use x86_64::instructions::tables::load_tss;
    use x86_64::instructions::segmentation::{CS, DS};

    GDT.0.load();
    serial_println!("Global Descriptor Table defined!");

    unsafe {
        CS::set_reg(GDT.1.code_selector);
        DS::set_reg(GDT.1.data_selector);
        load_tss(GDT.1.tss_selector);
    }

    serial_println!("CS, DS and TSS loaded!");
}

pub fn get_kernel_segments() -> (SegmentSelector, SegmentSelector) {
    (GDT.1.code_selector, GDT.1.data_selector)
  }

  pub fn get_user_segments() -> (SegmentSelector, SegmentSelector) {
    (GDT.1.user_code_selector, GDT.1.user_data_selector)
}