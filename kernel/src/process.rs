extern crate alloc;
use alloc::vec::Vec;
use spin::RwLock;
use lazy_static::lazy_static;
use alloc::{boxed::Box, collections::vec_deque::VecDeque};
use x86_64::{instructions::interrupts, structures::paging::{FrameAllocator, Mapper, PageTableFlags, Size4KiB}, VirtAddr};
use object::{Object, ObjectSegment};

use crate::{gdt, memory};

#[derive(Debug)]
#[repr(packed)]
pub struct Context {
    // These are pushed in the handler function
    pub r15: usize,
    pub r14: usize,
    pub r13: usize,

    pub r12: usize,
    pub r11: usize,
    pub r10: usize,
    pub r9: usize,

    pub r8: usize,
    pub rbp: usize,
    pub rsi: usize,
    pub rdi: usize,

    pub rdx: usize,
    pub rcx: usize,
    pub rbx: usize,
    pub rax: usize,
    // Below is the exception stack frame pushed by the CPU on interrupt
    // Note: For some interrupts (e.g. Page fault), an error code is pushed here
    rip: usize,     // Instruction pointer
    cs: usize,      // Code segment
    rflags: usize,  // Processor flags
    rsp: usize,     // Stack pointer
    ss: usize,      // Stack segment
    // Here the CPU may push values to align the stack on a 16-byte boundary (for SSE)
}

pub fn schedule_next(context_addr: usize) -> usize {
    let mut running_queue = RUNNING_QUEUE.write();
    let mut current_thread = CURRENT_THREAD.write();

    if let Some(mut thread) = current_thread.take() {
        // Save the location of the Context struct
        thread.context = context_addr as u64;
        // Put to the back of the queue
        running_queue.push_back(thread);
    }
    // Get the next thread in the queue
    *current_thread = running_queue.pop_front();
    match current_thread.as_ref() {
        Some(thread) => {
            // Set the kernel stack for the next interrupt
            gdt::set_interrupt_stack_table(
              gdt::TIMER_INTERRUPT_INDEX as usize,
              VirtAddr::new(thread.kernel_stack_end));
            // Point the stack to the new context
            thread.context as usize
          },
        None => 0  // Timer handler won't modify stack
    }
}

lazy_static! {
    static ref RUNNING_QUEUE: RwLock<VecDeque<Box<Thread>>> =
        RwLock::new(VecDeque::new());

    static ref CURRENT_THREAD: RwLock<Option<Box<Thread>>> =
        RwLock::new(None);
}

struct Thread {
    kernel_stack: Vec<u8>,
    user_stack: Vec<u8>,
    kernel_stack_end: u64, // This address goes in the TSS
    user_stack_end: u64,
    context: u64, // Address of Context on kernel stack
}

const KERNEL_STACK_SIZE: usize = 4096 * 2;
const USER_STACK_SIZE: usize = 4096 * 5;
const INTERRUPT_CONTEXT_SIZE: usize = 40 + 120; // = 160 bytes
const USER_CODE_START: u64 = 0x5000000;
const USER_CODE_END: u64 = 0x80000000;
const USER_STACK_START: u64 = 0x5002000;

pub fn new_user_thread(bin: &[u8], mapper: &mut impl Mapper<Size4KiB>, frame_allocator: &mut impl FrameAllocator<Size4KiB>) -> Result<usize, &'static str> {
    // Check the header
    const ELF_MAGIC: [u8; 4] = [0x7f, b'E', b'L', b'F'];

    if bin[0..4] != ELF_MAGIC {
        return Err("Expected ELF binary");
    }
    // Use the object crate to parse the ELF file
    // https://crates.io/crates/object
    if let Ok(obj) = object::File::parse(bin) {
        let entry_point = obj.entry();

        for segment in obj.segments() {
            let segment_address = segment.address() as u64;
        
            kprintln!("Section {:?} : {:#016X}", segment.name(), segment_address);
        
            let start_address = VirtAddr::new(segment_address);
            let end_address = start_address + segment.size() as u64;
            if (start_address < VirtAddr::new(USER_CODE_START))
                || (end_address >= VirtAddr::new(USER_CODE_END)) {
                    return Err("ELF segment outside allowed range");
                }

            // Allocate memory in the pagetable
            if memory::allocate_pages_mapper(
                mapper,
                frame_allocator,
                VirtAddr::new(segment_address), // Start address
                segment.size() as u64, // Size (bytes)
                PageTableFlags::PRESENT |
                PageTableFlags::WRITABLE |
                PageTableFlags::USER_ACCESSIBLE).is_err() {
                return Err("Could not allocate memory");
            }
        
            if let Ok(data) = segment.data() {
                // Copy data
                let dest_ptr = segment_address as *mut u8;
                for (i, value) in data.iter().enumerate() {
                    unsafe {
                        let ptr = dest_ptr.add(i);
                        core::ptr::write(ptr, *value);
                    }
                }
            }
        }

        // Create the Thread object
        let new_thread = {
            let kernel_stack = Vec::with_capacity(KERNEL_STACK_SIZE);
            let kernel_stack_end = (VirtAddr::from_ptr(kernel_stack.as_ptr())
                                   + KERNEL_STACK_SIZE as u64).as_u64();
            let user_stack = Vec::with_capacity(USER_STACK_SIZE);
            let user_stack_end = (VirtAddr::from_ptr(user_stack.as_ptr())
                                  + USER_STACK_SIZE as u64).as_u64();
            let context = kernel_stack_end - INTERRUPT_CONTEXT_SIZE as u64;

            Box::new(Thread {
                kernel_stack,
                user_stack,
                kernel_stack_end,
                user_stack_end,
                context,
            })
        };

        // Set context registers
        let context = unsafe { &mut *(new_thread.context as *mut Context) };
        context.rip = entry_point as usize; // Instruction pointer
        memory::allocate_pages_mapper(
            mapper,
            frame_allocator,
            VirtAddr::new(USER_STACK_START), // Start address
            USER_STACK_SIZE as u64, // Size (bytes)
            PageTableFlags::PRESENT |
            PageTableFlags::WRITABLE |
            PageTableFlags::USER_ACCESSIBLE);
        context.rsp = (USER_STACK_START as usize) + USER_STACK_SIZE; // Stack pointer
        context.rflags = 0x200; // Interrupts enabled

        let (code_selector, data_selector) = gdt::get_user_segments();
        context.cs = code_selector.0 as usize;
        context.ss = data_selector.0 as usize;

        interrupts::without_interrupts(|| {
            RUNNING_QUEUE.write().push_back(new_thread);
        });

        return Ok(entry_point as usize);
    }
    Err("Could not parse ELF")
}

pub fn new_kernel_thread(function: fn()->()) {
    let new_thread = {
        let kernel_stack = Vec::with_capacity(KERNEL_STACK_SIZE);
        let kernel_stack_end = (VirtAddr::from_ptr(kernel_stack.as_ptr())
                               + KERNEL_STACK_SIZE as u64).as_u64();
        let user_stack = Vec::with_capacity(USER_STACK_SIZE);
        let user_stack_end = (VirtAddr::from_ptr(user_stack.as_ptr())
                              + USER_STACK_SIZE as u64).as_u64();
        let context = kernel_stack_end - INTERRUPT_CONTEXT_SIZE as u64;

        Box::new(Thread {
            kernel_stack,
            user_stack,
            kernel_stack_end,
            user_stack_end,
            context})
    };
    // Set context registers
    // Add Thread to RUNNING_QUEUE
    let context = unsafe {&mut *(new_thread.context as *mut Context)};
    context.rip = function as usize; // Instruction pointer
    context.rsp = new_thread.user_stack_end as usize; // Stack pointer
    context.rflags = 0x200; // Interrupts enabled

    let (code_selector, data_selector) = gdt::get_kernel_segments();
    context.cs = code_selector.0 as usize;
    context.ss = data_selector.0 as usize;

    interrupts::without_interrupts(|| {
        RUNNING_QUEUE.write().push_back(new_thread);
    });
}