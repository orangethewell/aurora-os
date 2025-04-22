use alloc::string::{String, ToString};
use x86_64::instructions::port::Port;
use simple_fatfs::*;
use simple_fatfs::io::prelude::*;
use core::arch::asm;

#[derive(Debug)]
pub struct IdeDevice {
    pub channel: &'static str,
    pub drive: &'static str,
    pub model: [u8; 40],
}

#[inline(always)]
fn io_wait() {
    unsafe { asm!("out 0x80, al", in("al") 0u8); }
}

pub fn detect_ide_devices() -> [Option<IdeDevice>; 4] {
    let channels = [
        ("Primário", 0x1F0, 0x3F6),
        ("Secundário", 0x170, 0x376),
    ];

    let mut devices: [Option<IdeDevice>; 4] = [None, None, None, None];

    for (channel_idx, &(channel_name, io_base, _ctrl_base)) in channels.iter().enumerate() {
        for drive_idx in 0..2 {
            let is_master = drive_idx == 0;
            let drive_name = if is_master { "Master" } else { "Slave" };
            let select = if is_master { 0xA0 } else { 0xB0 };

            unsafe {
                let mut port_drive = Port::<u8>::new(io_base + 6);
                let mut port_status = Port::<u8>::new(io_base + 7);
                let mut port_command = Port::<u8>::new(io_base + 7);
                let mut port_data = Port::<u16>::new(io_base + 0);

                // Selecione o drive
                port_drive.write(select);
                io_wait();

                // Zere os registradores de setor
                Port::new(io_base + 2).write(0u8); // Sector Count
                Port::new(io_base + 3).write(0u8); // LBA Low
                Port::new(io_base + 4).write(0u8); // LBA Mid
                Port::new(io_base + 5).write(0u8); // LBA High

                // Comando IDENTIFY
                port_command.write(0xEC);
                io_wait();

                // Verifique se há dispositivo
                let status = port_status.read();
                if status == 0 {
                    continue; // Nada conectado
                }

                // Aguarde até que DRQ esteja setado e BSY limpo
                loop {
                    let s = port_status.read();
                    if s & 0x80 != 0 { continue; } // BSY
                    if s & 0x08 != 0 { break; }    // DRQ
                    if s & 0x01 != 0 { break; }    // ERR
                }

                // Leia os 256 words (512 bytes)
                let mut identify_data = [0u16; 256];
                for word in identify_data.iter_mut() {
                    *word = port_data.read();
                }

                // Modelo está nos words 27–46
                let mut model_bytes = [0u8; 40];
                for (i, word) in identify_data[27..47].iter().enumerate() {
                    model_bytes[i * 2] = (word >> 8) as u8;
                    model_bytes[i * 2 + 1] = (*word & 0xFF) as u8;
                }

                let index = channel_idx * 2 + drive_idx;
                devices[index] = Some(IdeDevice {
                    channel: channel_name,
                    drive: drive_name,
                    model: model_bytes,
                });
            }
        }
    }

    devices
}

/// Lê um setor (512 bytes) do canal IDE primário ou secundário.
/// `channel_base` = 0x1F0 (primário) ou 0x170 (secundário)
/// `lba`: setor lógico (48‑bit, mas aqui só usa 28 bits)
/// `buffer`: &mut [u8;512]
pub fn read_sector(channel_base: u16, lba: u32, buffer: &mut [u8;512]) -> Result<(), ()> {
    let ctrl_base = if channel_base == 0x1F0 { 0x3F6 } else { 0x376 };

    unsafe {
        // Seleciona drive Master no canal
        Port::<u8>::new(channel_base as u16 + 6).write(0xE0 | ((lba >> 24) & 0x0F) as u8);
        io_wait();

        // Preenche registradores
        Port::<u8>::new(channel_base + 2).write(1);             // sector count = 1
        Port::<u8>::new(channel_base + 3).write((lba & 0xFF) as u8);
        Port::<u8>::new(channel_base + 4).write(((lba >> 8) & 0xFF) as u8);
        Port::<u8>::new(channel_base + 5).write(((lba >> 16) & 0xFF) as u8);

        // Envia comando READ SECTOR (0x20)
        Port::<u8>::new(channel_base + 7).write(0x20);
        io_wait();

        // Poll até DRQ=1 e BSY=0
        let mut status;
        loop {
            status = Port::<u8>::new(channel_base + 7).read();
            if status & 0x80 == 0 && status & 0x08 != 0 { break; }
        }

        // Lê 256 palavras de 16‐bits = 512 bytes
        let mut data = Port::<u16>::new(channel_base);
        let ptr = buffer.as_mut_ptr() as *mut u16;
        for i in 0..256 {
            let w = data.read();
            core::ptr::write_volatile(ptr.add(i as usize), w);
        }
    }

    Ok(())
}

/// Escreve um setor (512 bytes) no canal IDE.
/// Mesma assinatura de `read_sector`, mas envia comando WRITE (0x30).
pub fn write_sector(channel_base: u16, lba: u32, buffer: &[u8;512]) -> Result<(), ()> {
    let ctrl_base = if channel_base == 0x1F0 { 0x3F6 } else { 0x376 };

    unsafe {
        Port::<u8>::new(channel_base + 6).write(0xE0 | ((lba >> 24) & 0x0F) as u8);
        io_wait();

        Port::<u8>::new(channel_base + 2).write(1);
        Port::<u8>::new(channel_base + 3).write((lba & 0xFF) as u8);
        Port::<u8>::new(channel_base + 4).write(((lba >> 8) & 0xFF) as u8);
        Port::<u8>::new(channel_base + 5).write(((lba >> 16) & 0xFF) as u8);

        Port::<u8>::new(channel_base + 7).write(0x30);
        io_wait();

        // Poll até DRQ pronto
        loop {
            let s = Port::<u8>::new(channel_base + 7).read();
            if s & 0x80 == 0 && s & 0x08 != 0 { break; }
        }

        // Escreve 256 palavras de 16‐bits
        let data = buffer.as_ptr() as *const u16;
        let mut port_data = Port::<u16>::new(channel_base);
        for i in 0..256 {
            let w = core::ptr::read_volatile(data.add(i));
            port_data.write(w);
        }
    }

    Ok(())
}

/// Tamanho fixo de cada setor em bytes
const SECTOR_SIZE: usize = 512;

/// Um "device" que o simple-fatfs pode usar.
/// Internamente faz read/write de setores via PIO IDE.
pub struct IdeBlockDevice {
    /// LBA de início da partição (boot sector)
    lba_start: u64,
    /// Posição atual de cursor, em bytes
    pos: u64,
}

impl IdeBlockDevice {
    /// Cria um novo bloco iniciando na LBA `lba_start`.
    pub fn new(lba_start: u64) -> Self {
        Self { lba_start, pos: 0 }
    }
}

pub struct IDEError {
    kind: IDEErrorKind,
    message: Option<String>,
}

#[derive(Debug, PartialEq)]
pub enum IDEErrorKind {
    General,
    NotFound,
    PermissionDenied,
    UnexpectedEOF,
    Interrupted,
    InvalidData,
    // Add other error kinds as needed
}

impl IDEError {
    pub fn new(kind: IDEErrorKind, message: Option<String>) -> Self {
        Self { kind, message }
    }

    pub fn kind(&self) -> &IDEErrorKind {
        &self.kind
    }

    pub fn message(&self) -> Option<&String> {
        self.message.as_ref()
    }
}

impl IOErrorKind for IDEErrorKind {
    fn new_unexpected_eof() -> Self {
        Self::UnexpectedEOF
    }

    fn new_interrupted() -> Self {
        Self::Interrupted
    }

    fn new_invalid_data() -> Self {
        Self::InvalidData
    }
}

impl core::fmt::Display for IDEError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{:?}", self.kind)?;
        if let Some(msg) = &self.message {
            write!(f, ": {}", msg)?;
        }
        Ok(())
    }
}

impl Error for IDEError {}

impl core::fmt::Debug for IDEError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        self.fmt(f)
    }
}

impl IOError for IDEError {
    type Kind = IDEErrorKind;

    fn new<M>(kind: Self::Kind, msg: M) -> Self
    where
        M: core::fmt::Display {
        Self {
            kind,
            message: Some(msg.to_string())
        }
    }

    fn kind(&self) -> Self::Kind {
        todo!()
    }
}

impl IOBase for IdeBlockDevice {
    type Error = IDEError;
}

/// Estrutura simples para uma entrada de partição no MBR
#[derive(Debug, Clone, Copy)]
pub struct PartitionEntry {
    pub boot_flag:   u8,
    pub chs_start:   [u8; 3],
    pub part_type:   u8,
    pub chs_end:     [u8; 3],
    pub lba_start:   u32,
    pub num_sectors: u32,
}

/// Lê o setor 0 (MBR) e retorna as 4 entradas de partição
pub fn read_partition_table() -> [PartitionEntry; 4] {
    let mut mbr = [0u8; 512];
    // canal primário master, LBA 0
    crate::ide::read_sector(0x1F0, 0, &mut mbr).unwrap();

    let mut parts = [PartitionEntry {
        boot_flag:   0,
        chs_start:   [0;3],
        part_type:   0,
        chs_end:     [0;3],
        lba_start:   0,
        num_sectors: 0,
    }; 4];

    for i in 0..4 {
        let off = 446 + i * 16;
        parts[i] = PartitionEntry {
            boot_flag:   mbr[off],
            chs_start:   [mbr[off+1], mbr[off+2], mbr[off+3]],
            part_type:   mbr[off+4],
            chs_end:     [mbr[off+5], mbr[off+6], mbr[off+7]],
            lba_start:   u32::from_le_bytes([mbr[off+8], mbr[off+9], mbr[off+10], mbr[off+11]]),
            num_sectors: u32::from_le_bytes([mbr[off+12], mbr[off+13], mbr[off+14], mbr[off+15]]),
        };
    }
    parts
}


impl Read for IdeBlockDevice {
    fn read(&mut self, buf: &mut [u8]) -> Result<usize, IDEError> {
        // Calcule qual setor e offset interno
        let sector_idx = (self.pos / SECTOR_SIZE as u64) as u32;
        let offset = (self.pos % SECTOR_SIZE as u64) as usize;
        let mut sector = [0u8; SECTOR_SIZE];
        read_sector(0x1F0, self.lba_start as u32 + sector_idx, &mut sector)
            .map_err(|_| IDEError::new(IDEErrorKind::General, Some("Something Wrong".to_string())))?;
        // Copia a parte relevante
        let to_copy = core::cmp::min(buf.len(), SECTOR_SIZE - offset);
        buf[..to_copy].copy_from_slice(&sector[offset..offset + to_copy]);
        self.pos += to_copy as u64;
        Ok(to_copy)
    }
}

impl Write for IdeBlockDevice {
    fn write(&mut self, buf: &[u8]) -> Result<usize, IDEError> {
        let sector_idx = (self.pos / SECTOR_SIZE as u64) as u32;
        let offset = (self.pos % SECTOR_SIZE as u64) as usize;
        let mut sector = [0u8; SECTOR_SIZE];
        // Primeiro lê o setor inteiro se for um write parcial
        read_sector(0x1F0, self.lba_start as u32 + sector_idx, &mut sector)
            .map_err(|_| IDEError::new(IDEErrorKind::General, Some("Something Wrong".to_string())))?;
        let to_copy = core::cmp::min(buf.len(), SECTOR_SIZE - offset);
        sector[offset..offset + to_copy].copy_from_slice(&buf[..to_copy]);
        write_sector(0x1F0, self.lba_start as u32 + sector_idx, &sector)
            .map_err(|_| IDEError::new(IDEErrorKind::General, Some("Something Wrong".to_string())))?;
        self.pos += to_copy as u64;
        Ok(to_copy)
    }

    fn flush(&mut self) -> Result<(), IDEError> {
        Ok(())
    }
}

impl Seek for IdeBlockDevice {
    fn seek(&mut self, pos: SeekFrom) -> Result<u64, IDEError> {
        let new = match pos {
            SeekFrom::Start(o) => o,
            SeekFrom::Current(o) => (self.pos as i64 + o) as u64,
            SeekFrom::End(o) => {
                // Não implementado: só suportar Start/Current
                self.pos
            }
        };
        self.pos = new;
        Ok(self.pos)
    }
}

/// Monta o sistema de arquivos FAT e demonstra leitura do diretório raiz.
pub fn mount_and_list(lba_start: u64) {
    // Cria o dispositivo de bloco iniciando na partição LBA
    let mut dev = IdeBlockDevice::new(lba_start);

    // Monta o filesystem FAT (detecta FAT12/16/32) :contentReference[oaicite:1]{index=1}
    let mut fs = FileSystem::from_storage(&mut dev).unwrap();

    // Lê e imprime cada entry no diretório raiz
    let entries = fs.read_dir(PathBuf::from("/")).unwrap();
    for entry in entries {
        if entry.path().is_dir() {
            kprintln!("Dir: {:?}", entry.path());
        } else {
            kprintln!("File: {:?} ({} bytes)", 
                entry.path(), 
                entry.file_size()
            );
        }
    }
}