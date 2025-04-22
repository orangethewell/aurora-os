#![no_std]
#![no_main]
#![feature(abi_x86_interrupt)]
#![feature(naked_functions)]

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
mod process;
mod syscall;

mod ide;

use core::{arch::asm, panic::PanicInfo};

use bootloader_api::{config::Mapping, BootloaderConfig};
use memory::BootInfoFrameAllocator;
use task::{executor::Executor, Task};
use x86_64::{instructions::port::Port, VirtAddr};

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


fn kernel_thread_main() {
    kprintln!("Kernel thread start");

    // Launch another kernel thread
    process::new_kernel_thread(test_kernel_fn2);

    loop {
        kprintln!("<< 1 >>");
        x86_64::instructions::hlt();
    }
}

fn test_kernel_fn2() {
    kprintln!("Hello from kernel function 2!");
    loop {
        kprintln!("       << 2 >>");
        x86_64::instructions::hlt();
    }
}

unsafe fn pci_config_read(bus: u8, device: u8, function: u8, offset: u8) -> u32 {
    let address: u32 =
        (1 << 31) | // habilita
        ((bus as u32) << 16) |
        ((device as u32) << 11) |
        ((function as u32) << 8) |
        ((offset as u32) & 0xFC);

    let mut port_cf8 = Port::new(0xCF8);
    let mut port_cfc = Port::new(0xCFC);
    port_cf8.write(address);
    port_cfc.read()
}

pub unsafe fn scan_pci() {
    for bus in 0..=255 {
        for device in 0..32 {
            for function in 0..8 {
                let data = pci_config_read(bus, device, function, 0);
                let vendor_id = (data & 0xFFFF) as u16;
                let device_id = ((data >> 16) & 0xFFFF) as u16;

                if vendor_id != 0xFFFF {
                    kprintln!(
                        "PCI Device encontrado: Bus {:02x}, Dev {:02x}, Func {:x} => Vendor {:04x}, Device {:04x}",
                        bus, device, function, vendor_id, device_id
                    );
                }

                // Apenas a função 0 existe, a menos que seja um dispositivo multifunção
                if function == 0 {
                    let header_type = (pci_config_read(bus, device, function, 0x0C) >> 16) & 0xFF;
                    if (header_type & 0x80) == 0 {
                        break;
                    }
                }
            }
        }
    }
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

    unsafe { scan_pci();}
    for device in ide::detect_ide_devices().iter().flatten() {
        let model_str = core::str::from_utf8(&device.model).unwrap_or("???").trim();
        kprintln!(
            "Dispositivo IDE: {} {} - Modelo: {}",
            device.channel,
            device.drive,
            model_str
        );
    }

    process::new_user_thread(
        include_bytes!("../../target/x86_64-unknown-none/debug/hello"),
        &mut mapper,
        &mut frame_allocator
    );

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