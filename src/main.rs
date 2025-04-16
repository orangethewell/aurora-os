use std::{env, fs, path::Path};

fn main() {
    // Obter o diretório do executável atual
    let current_exe = env::current_exe().unwrap();

    // Define os caminhos dos arquivos UEFI e BIOS
    let uefi_target = current_exe.with_file_name("uefi.img");
    let bios_target = current_exe.with_file_name("bios.img");

    // Define o caminho do diretório de destino
    let target_dir = Path::new("/mnt/c/WSLInternals");

    // Cria o diretório "WSLInternals" se não existir
    if !target_dir.exists() {
        fs::create_dir_all(target_dir).expect("Falha ao criar o diretório WSLInternals");
    }

    // Caminhos de destino no Windows (quando rodando a partir do WSL)
    let uefi_windows_path = target_dir.join("uefi.img");
    let bios_windows_path = target_dir.join("bios.img");

    // Copia os arquivos de BIOS e UEFI para o diretório Windows/WSL
    fs::copy(env!("UEFI_IMAGE"), &uefi_windows_path).unwrap();
    fs::copy(env!("BIOS_IMAGE"), &bios_windows_path).unwrap();

    fs::copy(env!("UEFI_IMAGE"), &uefi_target).unwrap();
    fs::copy(env!("BIOS_IMAGE"), &bios_target).unwrap();

    
    // Imprime os caminhos das imagens copiadas
    println!("UEFI disk image at {}", uefi_target.display());
    println!("BIOS disk image at {}", bios_target.display());

    println!("UEFI disk image on windows at {}", uefi_windows_path.display());
    println!("BIOS disk image on windows at {}", bios_windows_path.display());
}