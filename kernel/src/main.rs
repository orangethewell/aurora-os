#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]

#[macro_use]
extern crate alloc;

#[macro_use]
mod serial;
#[macro_use]
mod tty;
mod framebuffer;

mod gdt;
mod interrupts;
mod memory;
mod allocator;

mod task;

use core::panic::PanicInfo;

use bootloader_api::{config::Mapping, BootloaderConfig};
use memory::BootInfoFrameAllocator;
use task::{executor::Executor, simple_executor::SimpleExecutor, Task};
use x2apic::lapic::{xapic_base, LocalApicBuilder};
use x86_64::{structures::paging::Translate, VirtAddr};

pub const BOOTLOADER_CONFIG: BootloaderConfig = {
    let mut config = BootloaderConfig::new_default();
    config.mappings.physical_memory = Some(Mapping::FixedAddress(0x0000_4000_0000_0000));
    config
};

bootloader_api::entry_point!(kernel_main, config=&BOOTLOADER_CONFIG);

async fn async_number() -> u32 {
    42
}

async fn example_task() {
    let number = async_number().await;
    kprintln!("async number: {}", number);
}

fn kernel_main(boot_info: &'static mut bootloader_api::BootInfo) -> ! {
    gdt::init();
    interrupts::init_idt();

    serial_println!("Loading memory mapping and frame allocator...");

    let physical_memory_offset = boot_info.physical_memory_offset.into_option().unwrap();
    let phys_mem_offset = VirtAddr::new(physical_memory_offset );
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {BootInfoFrameAllocator::init(&boot_info.memory_regions)};
    serial_println!("Loaded!");
    allocator::init_heap(&mut mapper, &mut frame_allocator)
    .expect("heap initialization failed");
    serial_println!("Heap initialized!");

    let rsdp: Option<u64> = boot_info.rsdp_addr.take();

    unsafe {
        interrupts::disable_pic();
        interrupts::init_apic(rsdp.expect("Couldn't get rsdp addr.") as usize, phys_mem_offset, &mut mapper, &mut frame_allocator);
    }

    serial_println!("APIC (IO|LAPIC) initialized!");

    let fb_info = boot_info.framebuffer.as_ref().unwrap();
    let fb_addr = VirtAddr::new(fb_info.buffer().as_ptr() as u64);
    let fb_size = fb_info.buffer().len();

    let fb_buf = unsafe {
        framebuffer::remap_framebuffer_with_wc(
            fb_addr,
            fb_size,
            &mut mapper, 
        )
    };

    // let ptr = fb_addr.as_mut_ptr::<u8>();
    // let fb_buf = unsafe { slice::from_raw_parts_mut(ptr, fb_size) } ;

    serial_println!("Framebuffer with WC loaded!");

    x86_64::instructions::interrupts::enable();    
    serial_println!("System interrupts enabled!");

    let display = framebuffer::Display::new_from_buffer(fb_buf, &fb_info.info());
    let tty0 = tty::TTY::new(display);
    tty::activate_tty(tty0);
    kprintln!("TTY Initialized!");

    let mut executor = Executor::new();
    executor.spawn(Task::new(example_task()));
    executor.spawn(Task::new(task::keyboard::print_keypresses())); // new
    executor.run();

    kprintln!("Welcome to Aurora OS!");

    hlt_loop()
}

pub fn hlt_loop() -> ! {
    loop {
        x86_64::instructions::hlt();
    }
}

/// This function is called on panic.
#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    kprintln!("{}", info);
    hlt_loop();
}