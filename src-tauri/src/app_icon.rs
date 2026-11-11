use std::mem::size_of;
use std::path::{Component, Path, Prefix};
use std::ptr;

use base64::{Engine as _, engine::general_purpose::STANDARD};
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS,
    DeleteDC, DeleteObject, GdiFlush, SelectObject,
};
use windows::Win32::UI::Shell::ExtractIconExW;
use windows::Win32::UI::WindowsAndMessaging::{DI_NORMAL, DestroyIcon, DrawIconEx, HICON};
use windows::core::PCWSTR;

const SIZE: i32 = 32;
const BYTE_COUNT: usize = (SIZE * SIZE * 4) as usize;

pub fn from_exe(path: &str) -> Option<String> {
    let file = Path::new(path);
    let local = matches!(file.components().next(), Some(Component::Prefix(prefix))
        if matches!(prefix.kind(), Prefix::Disk(_)));
    if !local || !file.is_absolute() || !file.extension()?.eq_ignore_ascii_case("exe") {
        return None;
    }
    let wide: Vec<u16> = path.encode_utf16().chain([0]).collect();
    let mut icon = HICON::default();
    let rgba = unsafe {
        ExtractIconExW(PCWSTR(wide.as_ptr()), 0, Some(&mut icon), None, 1);
        if icon.is_invalid() {
            return None;
        }
        let result = draw(icon);
        let _ = DestroyIcon(icon);
        result?
    };
    let mut png = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut png, SIZE as u32, SIZE as u32);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_compression(png::Compression::Fast);
        encoder.write_header().ok()?.write_image_data(&rgba).ok()?;
    }
    Some(format!("data:image/png;base64,{}", STANDARD.encode(png)))
}

fn draw(icon: HICON) -> Option<Vec<u8>> {
    let info = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SIZE,
            biHeight: -SIZE,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe {
        let mut bits = ptr::null_mut();
        let bitmap = CreateDIBSection(None, &info, DIB_RGB_COLORS, &mut bits, None, 0).ok()?;
        let dc = CreateCompatibleDC(None);
        let result = if dc.is_invalid() {
            None
        } else {
            let previous = SelectObject(dc, bitmap.into());
            let result = (|| {
                if previous.is_invalid() || bits.is_null() {
                    return None;
                }
                let pixels = bits.cast::<u8>();
                let render = |background| {
                    ptr::write_bytes(pixels, background, BYTE_COUNT);
                    let result = DrawIconEx(dc, 0, 0, icon, SIZE, SIZE, 0, None, DI_NORMAL);
                    let _ = GdiFlush();
                    result.ok()?;
                    Some(std::slice::from_raw_parts(pixels, BYTE_COUNT).to_vec())
                };
                let mut rgba = render(0)?;
                let white = render(255)?;
                // Recover transparency from the two backgrounds, including legacy icon masks.
                for (black, white) in rgba.chunks_exact_mut(4).zip(white.chunks_exact(4)) {
                    let alpha = 255 - white[1].saturating_sub(black[1]);
                    if alpha == 0 {
                        black.fill(0);
                    } else {
                        for value in &mut black[..3] {
                            *value = ((u16::from(*value) * 255) / u16::from(alpha)).min(255) as u8;
                        }
                        black.swap(0, 2);
                        black[3] = alpha;
                    }
                }
                Some(rgba)
            })();
            if !previous.is_invalid() {
                SelectObject(dc, previous);
            }
            let _ = DeleteDC(dc);
            result
        };
        let _ = DeleteObject(bitmap.into());
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_explorer_icon_with_transparency() {
        let path = format!(r"{}\explorer.exe", std::env::var("WINDIR").unwrap());
        let url = from_exe(&path).expect("Explorer icon");
        let bytes = STANDARD.decode(url.strip_prefix("data:image/png;base64,").unwrap()).unwrap();
        let mut reader = png::Decoder::new(std::io::Cursor::new(bytes)).read_info().unwrap();
        let mut pixels = vec![0; BYTE_COUNT];
        let frame = reader.next_frame(&mut pixels).unwrap();
        assert_eq!((frame.width, frame.height, frame.color_type), (32, 32, png::ColorType::Rgba));
        assert!(pixels.chunks_exact(4).any(|p| p[3] == 0));
        assert!(pixels.chunks_exact(4).any(|p| p[3] == 255));
    }
}
