use std::{
    env,
    process::{self, Command},
};

fn main() {
    let mut qemu = Command::new("/mnt/c/Program Files/qemu/qemu-system-x86_64.exe");
    qemu.arg("-drive");
    qemu.arg(format!("format=raw,file={}", "C:\\WSLInternals\\aurora-os-bios.img"));
    qemu.arg("-accel");
    qemu.arg("whpx");
    qemu.arg("-serial");
    qemu.arg("stdio");
    let exit_status = qemu.status().unwrap();
    process::exit(exit_status.code().unwrap_or(-1));
}