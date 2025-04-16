use core::{ptr, slice};
use alloc::boxed::Box;
use x86_64::{
    structures::paging::{Mapper, Page, PageTableFlags, Size4KiB},
    VirtAddr
};
use embedded_graphics::{
    Pixel,
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Size},
    pixelcolor::{Rgb888, RgbColor},
};

use bootloader_api::info::{PixelFormat, FrameBufferInfo};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Position {
    pub x: usize,
    pub y: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Color {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
}

pub struct Display<'a> {
    shadow: Box<[u8]>,
    buffer: &'a mut [u8],
    info: FrameBufferInfo,
}

/// Atualiza os flags da região mapeada do framebuffer para forçar write combining
/// e retorna um slice mutável para o framebuffer.
///
/// # Segurança
/// - O framebuffer já deve estar mapeado pelo bootloader.
/// - `framebuffer_virt_base` deve apontar para o início do mapeamento,
///   e `framebuffer_size` deve ser o tamanho total da região.
/// - Essa função atualiza os flags das entradas da tabela de páginas; use-a com cuidado.
pub unsafe fn remap_framebuffer_with_wc<'a>(
    framebuffer_virt_base: VirtAddr,
    framebuffer_size: usize,
    mapper: &mut impl Mapper<Size4KiB>,
) -> &'a mut [u8] {
    // Define o range de páginas que cobrem o framebuffer
    let start = framebuffer_virt_base;
    let end = framebuffer_virt_base + (framebuffer_size - 1) as u64;
    let start_page = Page::containing_address(start);
    let end_page = Page::containing_address(end);
    let page_range = Page::range_inclusive(start_page, end_page);

    // Define os novos flags para ativar Write Combining:
    // BIT_3 equivale a PWT e BIT_7 equivale ao bit PAT (para páginas de 4KiB, por exemplo)
    let wc_flags = PageTableFlags::PRESENT | PageTableFlags::WRITABLE | PageTableFlags::HUGE_PAGE;

    // Atualiza os flags de cada página no range
    for page in page_range {
        // update_flags atualiza os atributos da entrada de página sem desmapear
        mapper.update_flags(page, wc_flags).expect("Couldn't update framebuffer page flags.").flush();
    }

    // Cria um slice para o framebuffer mapeado
    let ptr = framebuffer_virt_base.as_mut_ptr::<u8>();
    slice::from_raw_parts_mut(ptr, framebuffer_size)
}



fn set_pixel_in(buf: &mut [u8], info: &FrameBufferInfo, position: Position, color: Color) {
    let byte_offset = {
        let line_offset = position.y * info.stride;
        let pixel_offset = line_offset + position.x;
        pixel_offset * info.bytes_per_pixel
    };

    let pixel_buffer = &mut buf[byte_offset..];
    match info.pixel_format {
        PixelFormat::Rgb => {
            pixel_buffer[0] = color.red;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.blue;
        }
        PixelFormat::Bgr => {
            pixel_buffer[0] = color.blue;
            pixel_buffer[1] = color.green;
            pixel_buffer[2] = color.red;
        }
        PixelFormat::U8 => {
            let gray = color.red / 3 + color.green / 3 + color.blue / 3;
            pixel_buffer[0] = gray;
        }
        other => panic!("unknown pixel format {other:?}"),
    }
}

impl<'a> Display<'a> {
    pub fn new_from_buffer(buffer: &'a mut [u8], info: &FrameBufferInfo) -> Self {
        let shadow = vec![0; buffer.len()].into_boxed_slice();
        Self { shadow, buffer, info: info.clone() }
    }

    pub fn flush(&mut self) {
        self.buffer.copy_from_slice(&self.shadow);
    }    

    pub fn clear_buf(&mut self) {
        unsafe {
            ptr::write_bytes(self.shadow.as_mut_ptr(), 0, self.buffer.len());
        }
    }

    pub fn draw_pixel(&mut self, Pixel(coordinates, color): Pixel<Rgb888>) {
        let (width, height) = (self.info.width, self.info.height);
        let (x, y) = {
            let c: (i32, i32) = coordinates.into();
            (c.0 as usize, c.1 as usize)
        };

        if (0..width).contains(&x) && (0..height).contains(&y) {
            let color = Color {
                red: color.r(),
                green: color.g(),
                blue: color.b(),
            };
            set_pixel_in(&mut self.shadow, &self.info, Position { x, y }, color);
        }
    }
}

impl<'a> DrawTarget for Display<'a> {
    type Color = Rgb888;
    type Error = core::convert::Infallible;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for pixel in pixels {
            self.draw_pixel(pixel);
        }

        Ok(())
    }
}

impl<'a> OriginDimensions for Display<'a> {
    fn size(&self) -> Size {
        Size::new(self.info.width as u32, self.info.height as u32)
    }
}