use core::{ptr::null_mut, slice};

use common_structures::KernelHeader;
use font8x8::UnicodeFonts;

const MARGIN: u32 = 16;

struct Info {
    framebuffer: *mut u8,
    scan_width: u32,
    width: u32,
    height: u32,

    rows: u32,
    columns: u32,
    cursor_x: u32,
    cursor_y: u32,
}

static mut INFO: Info = Info{
    framebuffer: null_mut(),
    scan_width: 0,
    width: 0,
    height: 0,
    rows: 0,
    columns: 0,
    cursor_x: 0,
    cursor_y: 0,
};

pub fn init(kernel_header: &KernelHeader) {
    unsafe {
        INFO = Info {
            framebuffer: kernel_header.screen_buffer,
            width: kernel_header.screen_width,
            height: kernel_header.screen_height,
            rows: (kernel_header.screen_height - MARGIN * 2) / 8,
            columns: (kernel_header.screen_width - MARGIN * 2) / 8,
            scan_width: kernel_header.screen_scanline_width,
            cursor_x: 0,
            cursor_y: 0,
        };
    }
}

pub fn clear() {
    let info = unsafe{&mut INFO};

    unsafe {
        info.framebuffer.write_bytes(0, (info.scan_width * info.height * 4) as usize);
    }
}

fn advance_cursor() {
    let info = unsafe{&mut INFO};

    info.cursor_x += 1;
    if info.cursor_x >= info.columns {
        info.cursor_y += 1;
        info.cursor_x = 0;
        if info.cursor_y >= info.rows {
            info.cursor_y = 0;
        }
    }
}

pub fn print_char(c: char) {
    if c == '\n' {
        advance_cursor();
        return;
    }

    let info = unsafe{&mut INFO};

    let glyph = { 
        let tmp = font8x8::BASIC_FONTS.get(c);
        if let Some(g) = tmp {
            g
        } else {
            font8x8::BASIC_FONTS.get(' ').unwrap()
        }
    };

    let x_start = MARGIN + info.cursor_x * 8;
    let y_start = MARGIN + info.cursor_y * 8;
    let fb = unsafe {slice::from_raw_parts_mut(info.framebuffer, (info.scan_width * info.height * 4) as usize)};

    for y in 0..8 {
        let row = glyph[y];

        for x in 0..8 {
            if row & (1 << x) == 0 {
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 0] = 0;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 1] = 0;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 2] = 0;
            } else {
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 0] = 255;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 1] = 255;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 2] = 255;
            }
        }
    }

    advance_cursor();
}

pub fn print(msg: &str) {
    for c in msg.chars() {
        print_char(c);
    }
}

pub struct TerminalStream {}

impl core::fmt::Write for TerminalStream {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        print(s);
        Ok(())
    }
}

static mut STREAM: TerminalStream = TerminalStream{};

pub fn stream() -> &'static mut TerminalStream {
    unsafe {
        &mut STREAM
    }
}
