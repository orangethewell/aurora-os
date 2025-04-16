// build.rs

use bootloader::DiskImageBuilder;
use std::{env, fs::File, path::PathBuf};
use std::io::Write;

fn main() {
    let kernel_path = env::var("CARGO_BIN_FILE_KERNEL").unwrap();
    let system_name = env::var("CARGO_PKG_NAME").unwrap();

    let disk_builder = DiskImageBuilder::new(PathBuf::from(kernel_path));

    // specify output paths
    let out_dir = PathBuf::from("/mnt/c/WSLInternals");
    let uefi_path = out_dir.join(format!("{system_name}-uefi.img"));
    let bios_path = out_dir.join(format!("{system_name}-bios.img"));

    // create the disk images
    disk_builder.create_uefi_image(&uefi_path).unwrap();
    disk_builder.create_bios_image(&bios_path).unwrap();

    // pass the disk image paths via environment variables
    println!("cargo:rustc-env=UEFI_IMAGE={}", uefi_path.display());
    println!("cargo:rustc-env=BIOS_IMAGE={}", bios_path.display());
}