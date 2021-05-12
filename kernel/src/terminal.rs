use core::{ptr::null_mut, slice};

use common_structures::{Format, KernelHeader};
use font8x8::UnicodeFonts;

const MARGIN: u32 = 16;

struct Info {
    framebuffer: *mut u8,
    scan_width: u32,
    width: u32,
    height: u32,
    format: Format,

    rows: u32,
    columns: u32,
    cursor_x: u32,
    cursor_y: u32,

    color_r: u8,
    color_g: u8,
    color_b: u8,
    mode: Mode,
}

enum Mode {
    Print,
    SetR,
    SetG,
    SetB,
}

static mut INFO: Info = Info{
    framebuffer: null_mut(),
    scan_width: 0,
    width: 0,
    height: 0,
    format: Format::RGB,
    rows: 0,
    columns: 0,
    cursor_x: 0,
    cursor_y: 0,
    color_r: 255,
    color_g: 255,
    color_b: 255,
    mode: Mode::Print,
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
            color_r: 255,
            color_g: 255,
            color_b: 255,
            mode: Mode::Print,
            format: kernel_header.screen_format,
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

fn new_line() {
    let info = unsafe{&mut INFO};
    info.cursor_x = 0;
    info.cursor_y += 1;
    if info.cursor_y >= info.rows {
        info.cursor_y = 0;
    }
}

pub fn print_char(c: char) {
    let info = unsafe{&mut INFO};

    match info.mode {
        Mode::SetR => {
            info.color_r = c as u8;
            info.mode = Mode::SetG;
            return;
        }
        Mode::SetG => {
            info.color_g = c as u8;
            info.mode = Mode::SetB;
            return;
        }
        Mode::SetB => {
            info.color_b = c as u8;
            info.mode = Mode::Print;
            return;
        }
        _ => {}
    }

    if c == '\x1B' {
        info.mode = Mode::SetR;
        return;
    }

    if c == '\n' {
        new_line();
        return;
    }

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
            if row & (1 << x) != 0 {
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 0] = if info.format == Format::BGR { info.color_b } else { info.color_r };
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 1] = info.color_g;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 2] = if info.format == Format::BGR { info.color_r } else { info.color_b };
            } else {
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 0] = 0;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 1] = 0;
                fb[((x_start + x + (y_start + y as u32) * info.scan_width) * 4) as usize + 2] = 0;
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

#[cfg(feature="verbose-logging")]
macro_rules! verbose {
    ($ctx:literal, $fmt:literal $(, $args:expr)*) => {
        {
            use core::fmt::Write;
            writeln!(crate::terminal::stream(), concat!("\x1B\u{88}\u{88}\u{88}[{:^15}] ", $fmt, "\x1B\u{FF}\u{FF}\u{FF}"), $ctx $(, $args)*).unwrap();
        }
    };
}

#[cfg(not(feature="verbose-logging"))]
macro_rules! verbose {
    ($fmt:literal $(, $args:expr)*) => {
        
    };
}

macro_rules! info {
    ($ctx:literal, $fmt:literal $(, $args:expr)*) => {
        {
            use core::fmt::Write;
            writeln!(crate::terminal::stream(), concat!("\x1B\u{00}\u{FF}\u{00}[{:^15}] \x1B\u{FF}\u{FF}\u{FF}", $fmt), $ctx $(, $args)*).unwrap();
        }
    };
}

macro_rules! warning {
    ($ctx:literal, $fmt:literal $(, $args:expr)*) => {
        {
            use core::fmt::Write;
            writeln!(crate::terminal::stream(), concat!("\x1B\u{FF}\u{FF}\u{00}[{:^15}] ", $fmt, "\x1B\u{FF}\u{FF}\u{FF}"), $ctx $(, $args)*).unwrap();
        }
    };
}

macro_rules! error {
    ($ctx:literal, $fmt:literal $(, $args:expr)*) => {
        {
            use core::fmt::Write;
            writeln!(crate::terminal::stream(), concat!("\x1B\u{FF}\u{00}\u{00}[{:^15}] ", $fmt, "\x1B\u{FF}\u{FF}\u{FF}"), $ctx $(, $args)*).unwrap();
        }
    };
}
