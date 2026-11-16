#![allow(dead_code)]
use image::{DynamicImage, GenericImageView, RgbaImage};
use std::io::{Read, Write};
use std::path::Path;
use std::process::{Command, Stdio};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorMode {
    ColorKCMY,
    CompositeCMY,
    GrayscaleCMY,
    GrayscaleK,
}

impl ColorMode {
    pub fn uses_cmy_black(self) -> bool {
        matches!(self, Self::CompositeCMY | Self::GrayscaleCMY)
    }

    pub fn is_grayscale(self) -> bool {
        matches!(self, Self::GrayscaleCMY | Self::GrayscaleK)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintQuality {
    Draft150,
    Normal300,
    High600,
}

impl PrintQuality {
    pub fn dpi(self) -> u16 {
        match self {
            Self::Draft150 => 150,
            Self::Normal300 => 300,
            Self::High600 => 600,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaperSize {
    A4,
    Letter,
    Legal,
    Photo4x6,
    Photo5x7,
    A5,
}

impl PaperSize {
    pub fn pcl_page_code(self) -> u8 {
        match self {
            Self::Letter => 2,
            Self::Legal => 3,
            Self::A5 => 25,
            Self::A4 => 26,
            Self::Photo4x6 => 74,
            Self::Photo5x7 => 75,
        }
    }

    pub fn dimensions_inches(self) -> (f32, f32) {
        match self {
            Self::A4 => (8.27, 11.69),
            Self::Letter => (8.50, 11.00),
            Self::Legal => (8.50, 14.00),
            Self::Photo4x6 => (4.00, 6.00),
            Self::Photo5x7 => (5.00, 7.00),
            Self::A5 => (5.83, 8.27),
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::A4 => "A4 (210 × 297 mm)",
            Self::Letter => "Carta / US Letter (8.5 × 11 in)",
            Self::Legal => "Oficio / US Legal (8.5 × 14 in)",
            Self::Photo4x6 => "Foto 4 × 6 in (10 × 15 cm)",
            Self::Photo5x7 => "Foto 5 × 7 in (13 × 18 cm)",
            Self::A5 => "A5 (148 × 210 mm)",
        }
    }
}

#[derive(Debug, Clone)]
pub struct PclJobConfig {
    pub color_mode: ColorMode,
    pub quality: PrintQuality,
    pub paper_size: PaperSize,
    pub use_pcl3gui_header: bool,
    pub copies: u16,
}

impl Default for PclJobConfig {
    fn default() -> Self {
        Self {
            color_mode: ColorMode::ColorKCMY,
            quality: PrintQuality::Normal300,
            paper_size: PaperSize::A4,
            use_pcl3gui_header: true,
            copies: 1,
        }
    }
}

/// Encodes an RGBA image into an HP DeskJet 1110/1115 PCL3 / PCL3GUI raster print job
pub fn encode_image_to_pcl(img: &RgbaImage, cfg: &PclJobConfig) -> Vec<u8> {
    let mut out = Vec::with_capacity(256 * 1024);
    write_job_header(&mut out, cfg);
    encode_page_bitmap(&mut out, img.width(), img.height(), img.as_raw(), 4, cfg);
    write_job_footer(&mut out);
    out
}

pub fn write_job_header(out: &mut Vec<u8>, cfg: &PclJobConfig) {
    let dpi = cfg.quality.dpi();
    let page_code = cfg.paper_size.pcl_page_code();
    let copies = cfg.copies.max(1);
    let quality_code = match cfg.quality {
        PrintQuality::Draft150 => -1i8,
        PrintQuality::Normal300 => 0i8,
        PrintQuality::High600 => 1i8,
    };

    if cfg.use_pcl3gui_header {
        let _ = write!(
            out,
            "\x1b%-12345X@PJL JOB NAME=\"HP1115\"\n@PJL SET TIMEOUT=90\n@PJL ENTER LANGUAGE=PCL3GUI\n"
        );
        // Media source (7H), Plain media (0M), Print quality, Page size, Unit DPI
        let _ = write!(
            out,
            "\x1b&l7H\x1b&l0M\x1b*o{quality_code}M\x1b&l{page_code}A\x1b&u{dpi}D"
        );
    } else {
        // Pure DW-PCL / DeskJet PCL3 job header
        let paper_code = if cfg.color_mode.uses_cmy_black() { 3 } else { 0 };
        let _ = write!(out, "\x1bE");
        let _ = write!(
            out,
            "\x1b&l{page_code}a0o0L\x1b&l{paper_code}M\x1b&l0E\x1b&l{copies}X\x1b*o{quality_code}M"
        );
        let _ = write!(out, "\x1b*rC\x1b*t{dpi}R\x1b&u{dpi}D");
        match cfg.color_mode {
            ColorMode::GrayscaleK => {
                let _ = write!(out, "\x1b*r1U");
            }
            ColorMode::ColorKCMY | ColorMode::CompositeCMY | ColorMode::GrayscaleCMY => {
                let _ = write!(out, "\x1b*r-4U");
            }
        }
    }
}

pub fn encode_page_bitmap(
    out: &mut Vec<u8>,
    width: u32,
    height: u32,
    pixels: &[u8],
    bytes_per_pixel: usize,
    cfg: &PclJobConfig,
) {
    let w = width as usize;
    let h = height as usize;
    let row_bytes = (w + 7) / 8;
    let dpi = cfg.quality.dpi();

    if cfg.use_pcl3gui_header {
        let planes: u8 = match cfg.color_mode {
            ColorMode::GrayscaleK => 1,
            ColorMode::ColorKCMY | ColorMode::CompositeCMY | ColorMode::GrayscaleCMY => 4,
        };
        let _ = write!(out, "\x1b*p0Y\x1b*p0X\x1b*r{w}S");
        let payload_len = 2 + (planes as usize) * 6;
        let _ = write!(out, "\x1b*g{payload_len}W");
        out.push(0x02); // Format 2: Complex Direct Planar
        out.push(planes);
        for _ in 0..planes {
            out.extend_from_slice(&dpi.to_be_bytes());
            out.extend_from_slice(&dpi.to_be_bytes());
            out.extend_from_slice(&2u16.to_be_bytes()); // 2 levels (1-bit per plane)
        }
        let _ = write!(out, "\x1b*r1A\x1b*b2M");
    } else {
        // Position cursor at (0,0), set raster width, Start Raster Graphics at current cursor (1A), Mode 2 PackBits compression (2M)
        let _ = write!(out, "\x1b*p0Y\x1b*r{w}S\x1b*p0X\x1b*r1A\x1b*b2M");
    }

    match cfg.color_mode {
        ColorMode::GrayscaleK => {
            let mut err_curr = vec![0i16; w + 2];
            let mut err_next = vec![0i16; w + 2];
            let mut k_row = vec![0u8; row_bytes];
            let mut empty_lines = 0usize;

            for y in 0..h {
                k_row.fill(0);
                err_next.fill(0);
                let row_offset = y * w * bytes_per_pixel;

                for x in 0..w {
                    let px = row_offset + x * bytes_per_pixel;
                    let (r, g, b) = read_rgb(&pixels[px..], bytes_per_pixel);
                    // Luminance -> darkness (0 = white, 255 = black)
                    let luma = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as i16;
                    let darkness = (255 - luma) + err_curr[x + 1];
                    let bit = if darkness >= 128 { 1u8 } else { 0u8 };
                    if bit != 0 {
                        k_row[x >> 3] |= 0x80 >> (x & 7);
                    }
                    let quant = if bit != 0 { 255i16 } else { 0i16 };
                    let diff = darkness - quant;
                    err_curr[x + 2] += (diff * 7) >> 4;
                    err_next[x] += (diff * 3) >> 4;
                    err_next[x + 1] += (diff * 5) >> 4;
                    err_next[x + 2] += diff >> 4;
                }
                std::mem::swap(&mut err_curr, &mut err_next);

                let trimmed = trim_trailing_zeros(&k_row);
                if trimmed.is_empty() {
                    empty_lines += 1;
                } else {
                    if empty_lines > 0 {
                        let _ = write!(out, "\x1b*b{empty_lines}Y");
                        empty_lines = 0;
                    }
                    let compressed = packbits_encode(trimmed);
                    let _ = write!(out, "\x1b*b{}W", compressed.len());
                    out.extend_from_slice(&compressed);
                }
            }
        }
        ColorMode::ColorKCMY | ColorMode::CompositeCMY | ColorMode::GrayscaleCMY => {
            let pure_cmy = cfg.color_mode.uses_cmy_black();
            let gray_only = cfg.color_mode.is_grayscale();
            let mut err_c_curr = vec![0i16; w + 2];
            let mut err_c_next = vec![0i16; w + 2];
            let mut err_m_curr = vec![0i16; w + 2];
            let mut err_m_next = vec![0i16; w + 2];
            let mut err_y_curr = vec![0i16; w + 2];
            let mut err_y_next = vec![0i16; w + 2];
            let mut err_k_curr = vec![0i16; w + 2];
            let mut err_k_next = vec![0i16; w + 2];

            let mut k_row = vec![0u8; row_bytes];
            let mut c_row = vec![0u8; row_bytes];
            let mut m_row = vec![0u8; row_bytes];
            let mut y_row = vec![0u8; row_bytes];
            let mut empty_lines = 0usize;

            for y in 0..h {
                k_row.fill(0);
                c_row.fill(0);
                m_row.fill(0);
                y_row.fill(0);
                err_c_next.fill(0);
                err_m_next.fill(0);
                err_y_next.fill(0);
                err_k_next.fill(0);

                let row_offset = y * w * bytes_per_pixel;
                for x in 0..w {
                    let px = row_offset + x * bytes_per_pixel;
                    let (mut r, mut g, mut b) = read_rgb(&pixels[px..], bytes_per_pixel);
                    if gray_only {
                        let luma = ((r as u32 * 77 + g as u32 * 150 + b as u32 * 29) >> 8) as u8;
                        r = luma;
                        g = luma;
                        b = luma;
                    }

                    let c0 = 255 - r as i16;
                    let m0 = 255 - g as i16;
                    let y0 = 255 - b as i16;
                    let k0 = c0.min(m0).min(y0);

                    let (c_val, m_val, y_val, k_val) = if pure_cmy {
                        // Composite black exclusively from Cyan + Magenta + Yellow (K=0)
                        (c0, m0, y0, 0)
                    } else if k0 > 200
                        && (c0 - m0).abs() < 18
                        && (m0 - y0).abs() < 18
                    {
                        (0, 0, 0, k0)
                    } else {
                        let ucr = (k0 * 3) / 4;
                        (c0 - ucr, m0 - ucr, y0 - ucr, ucr)
                    };

                    if dither_step(c_val, x, &mut err_c_curr, &mut err_c_next) {
                        c_row[x >> 3] |= 0x80 >> (x & 7);
                    }
                    if dither_step(m_val, x, &mut err_m_curr, &mut err_m_next) {
                        m_row[x >> 3] |= 0x80 >> (x & 7);
                    }
                    if dither_step(y_val, x, &mut err_y_curr, &mut err_y_next) {
                        y_row[x >> 3] |= 0x80 >> (x & 7);
                    }
                    if dither_step(k_val, x, &mut err_k_curr, &mut err_k_next) {
                        k_row[x >> 3] |= 0x80 >> (x & 7);
                    }
                }

                std::mem::swap(&mut err_c_curr, &mut err_c_next);
                std::mem::swap(&mut err_m_curr, &mut err_m_next);
                std::mem::swap(&mut err_y_curr, &mut err_y_next);
                std::mem::swap(&mut err_k_curr, &mut err_k_next);

                let tk = trim_trailing_zeros(&k_row);
                let tc = trim_trailing_zeros(&c_row);
                let tm = trim_trailing_zeros(&m_row);
                let ty = trim_trailing_zeros(&y_row);

                if tk.is_empty() && tc.is_empty() && tm.is_empty() && ty.is_empty() {
                    empty_lines += 1;
                } else {
                    if empty_lines > 0 {
                        let _ = write!(out, "\x1b*b{empty_lines}Y");
                        empty_lines = 0;
                    }
                    let ck = packbits_encode(tk);
                    let cc = packbits_encode(tc);
                    let cm = packbits_encode(tm);
                    let cy = packbits_encode(ty);

                    let _ = write!(out, "\x1b*b{}V", ck.len());
                    out.extend_from_slice(&ck);
                    let _ = write!(out, "\x1b*b{}V", cc.len());
                    out.extend_from_slice(&cc);
                    let _ = write!(out, "\x1b*b{}V", cm.len());
                    out.extend_from_slice(&cm);
                    let _ = write!(out, "\x1b*b{}W", cy.len());
                    out.extend_from_slice(&cy);
                }
            }
        }
    }

    // End Raster Graphics + Form Feed
    let _ = write!(out, "\x1b*rC\x0c");
}

pub fn write_job_footer(out: &mut Vec<u8>) {
    let _ = write!(out, "\x1bE\x1b%-12345X@PJL EOJ\n\x1b%-12345X");
}

#[inline]
fn read_rgb(px: &[u8], bpp: usize) -> (u8, u8, u8) {
    match bpp {
        1 => (px[0], px[0], px[0]),
        3 => (px[0], px[1], px[2]),
        4 => {
            let a = px[3] as u16;
            if a == 255 {
                (px[0], px[1], px[2])
            } else {
                let r = ((px[0] as u16 * a + 255 * (255 - a)) / 255) as u8;
                let g = ((px[1] as u16 * a + 255 * (255 - a)) / 255) as u8;
                let b = ((px[2] as u16 * a + 255 * (255 - a)) / 255) as u8;
                (r, g, b)
            }
        }
        _ => (255, 255, 255),
    }
}

#[inline]
fn dither_step(val: i16, x: usize, curr: &mut [i16], next: &mut [i16]) -> bool {
    let v = (val + curr[x + 1]).clamp(-255, 512);
    let bit = v >= 128;
    let quant = if bit { 255i16 } else { 0i16 };
    let diff = v - quant;
    curr[x + 2] += (diff * 7) >> 4;
    next[x] += (diff * 3) >> 4;
    next[x + 1] += (diff * 5) >> 4;
    next[x + 2] += diff >> 4;
    bit
}

fn trim_trailing_zeros(slice: &[u8]) -> &[u8] {
    let mut end = slice.len();
    while end > 0 && slice[end - 1] == 0 {
        end -= 1;
    }
    &slice[..end]
}

/// TIFF PackBits (PCL Compression Mode 2) encoder
pub fn packbits_encode(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(data.len() + 16);
    let mut i = 0;
    let n = data.len();

    while i < n {
        // Check for a run of identical bytes
        let mut run_len = 1usize;
        while i + run_len < n && run_len < 128 && data[i + run_len] == data[i] {
            run_len += 1;
        }

        if run_len >= 2 {
            // Control byte: 257 - run_len (in two's complement -(run_len - 1))
            out.push((257 - run_len) as u8);
            out.push(data[i]);
            i += run_len;
        } else {
            // Literal run
            let lit_start = i;
            let mut lit_len = 0usize;
            while i < n && lit_len < 128 {
                if i + 2 < n && data[i] == data[i + 1] && data[i] == data[i + 2] {
                    break;
                }
                i += 1;
                lit_len += 1;
            }
            out.push((lit_len - 1) as u8);
            out.extend_from_slice(&data[lit_start..lit_start + lit_len]);
        }
    }
    out
}

/// Generates a rich 300 DPI diagnostic & color/alignment test page in pure Rust
pub fn render_rust_test_page(serial: &str, firmware: &str, cfg: &PclJobConfig) -> RgbaImage {
    // 2400 x 3150 pixels at 300 DPI (~8.0 x 10.5 inches printable area)
    let width = 2200u32;
    let height = 2900u32;
    let mut img = RgbaImage::from_pixel(width, height, image::Rgba([255, 255, 255, 255]));

    // 1. Outer border frame (double border)
    draw_rect_outline(&mut img, 40, 40, width - 80, height - 80, 6, [0, 0, 0, 255]);
    draw_rect_outline(
        &mut img,
        56,
        56,
        width - 112,
        height - 112,
        2,
        [0, 110, 190, 255],
    );

    // 2. Top Header Banner (HP Blue)
    draw_filled_rect(&mut img, 80, 80, width - 160, 170, [0, 114, 198, 255]);
    draw_text_bitmap(
        &mut img,
        120,
        110,
        "HP DESKJET INK ADVANTAGE 1115 - PAGINA DE PRUEBA",
        5,
        [255, 255, 255, 255],
    );
    draw_text_bitmap(
        &mut img,
        120,
        175,
        &format!("PRODUCTO: F5S21A  |  SERIE: {serial}  |  FIRMWARE: {firmware}"),
        3,
        [230, 245, 255, 255],
    );

    // 3. Section 1: CMYK Primary & Secondary Solid Color Swatches
    draw_text_bitmap(
        &mut img,
        90,
        300,
        "1. PRUEBA DE COLORES PRIMARIOS Y SECUNDARIOS",
        4,
        [0, 0, 0, 255],
    );

    let swatches: [(&str, [u8; 4]); 7] = [
        ("CIAN (C)", [0, 174, 239, 255]),
        ("MAGENTA (M)", [236, 0, 140, 255]),
        ("AMARILLO (Y)", [255, 242, 0, 255]),
        ("NEGRO (K)", [0, 0, 0, 255]),
        ("ROJO (M+Y)", [230, 30, 37, 255]),
        ("VERDE (C+Y)", [0, 166, 81, 255]),
        ("AZUL (C+M)", [46, 49, 146, 255]),
    ];

    let swatch_w = 260u32;
    let swatch_h = 220u32;
    for (idx, (label, color)) in swatches.iter().enumerate() {
        let sx = 90 + (idx as u32) * 290;
        let sy = 360;
        draw_filled_rect(&mut img, sx, sy, swatch_w, swatch_h, *color);
        draw_rect_outline(&mut img, sx, sy, swatch_w, swatch_h, 3, [0, 0, 0, 255]);
        draw_text_bitmap(&mut img, sx + 12, sy + swatch_h + 18, label, 3, [0, 0, 0, 255]);
    }

    // 4. Section 2: Continuous Halftone Gradients (K, C, M, Y)
    draw_text_bitmap(
        &mut img,
        90,
        660,
        "2. DEGRADADOS DE INTENSIDAD POR CANAL (0% - 100%)",
        4,
        [0, 0, 0, 255],
    );

    let grad_w = width - 180;
    let grad_h = 75u32;
    let channels: [(&str, [u8; 3]); 4] = [
        ("K", [0, 0, 0]),
        ("C", [0, 174, 239]),
        ("M", [236, 0, 140]),
        ("Y", [255, 235, 0]),
    ];

    for (i, (name, target)) in channels.iter().enumerate() {
        let gy = 720 + (i as u32) * 105;
        draw_text_bitmap(&mut img, 90, gy + 20, name, 4, [0, 0, 0, 255]);
        let bar_x = 150;
        let bar_w = grad_w - 60;
        for x in 0..bar_w {
            let t = x as f32 / (bar_w - 1) as f32;
            let r = (255.0 * (1.0 - t) + target[0] as f32 * t) as u8;
            let g = (255.0 * (1.0 - t) + target[1] as f32 * t) as u8;
            let b = (255.0 * (1.0 - t) + target[2] as f32 * t) as u8;
            for y in gy..(gy + grad_h) {
                img.put_pixel(bar_x + x, y, image::Rgba([r, g, b, 255]));
            }
        }
        draw_rect_outline(&mut img, bar_x, gy, bar_w, grad_h, 2, [0, 0, 0, 255]);
    }

    // 5. Section 3: Printhead Nozzle Comb & Cartridge Alignment Grid
    draw_text_bitmap(
        &mut img,
        90,
        1180,
        "3. PEINE DE INYECTORES Y REJILLA DE ALINEACION DE CASQUILLOS (K vs CMY)",
        4,
        [0, 0, 0, 255],
    );

    // Draw staggered nozzle check staircase for K, C, M, Y
    let nozzle_colors: [[u8; 4]; 4] = [
        [0, 0, 0, 255],
        [0, 174, 239, 255],
        [236, 0, 140, 255],
        [245, 215, 0, 255],
    ];
    for (ci, col) in nozzle_colors.iter().enumerate() {
        let base_x = 100 + (ci as u32) * 500;
        let base_y = 1250;
        draw_rect_outline(&mut img, base_x, base_y, 460, 320, 2, [120, 120, 120, 255]);
        for step in 0..32u32 {
            let sx = base_x + 20 + (step % 8) * 52;
            let sy = base_y + 20 + step * 9;
            draw_filled_rect(&mut img, sx, sy, 44, 4, *col);
        }
    }

    // 6. Section 4: Calibration Patterns A, B, C, D (Vertical & Horizontal Vernier Scales)
    draw_text_bitmap(
        &mut img,
        90,
        1630,
        "4. ESCALAS VERNIER DE ALINEACION BIDIRECCIONAL (PATRONES A - D)",
        4,
        [0, 0, 0, 255],
    );

    for group in 0..4u32 {
        let gy = 1700 + group * 220;
        let group_char = (b'A' + group as u8) as char;
        draw_text_bitmap(
            &mut img,
            90,
            gy + 40,
            &format!("GRUPO {group_char}"),
            4,
            [0, 0, 0, 255],
        );

        for idx in 1..=9u32 {
            let px = 310 + (idx - 1) * 195;
            draw_text_bitmap(&mut img, px + 35, gy, &format!("{idx}"), 3, [0, 0, 0, 255]);
            // Black reference lines
            draw_filled_rect(&mut img, px + 20, gy + 35, 4, 70, [0, 0, 0, 255]);
            draw_filled_rect(&mut img, px + 60, gy + 35, 4, 70, [0, 0, 0, 255]);
            // Cyan/Magenta vernier shifted by (idx - 5) pixels
            let shift = (idx as i32) - 5;
            let cx1 = (px as i32 + 20 + shift) as u32;
            let cx2 = (px as i32 + 60 + shift) as u32;
            draw_filled_rect(&mut img, cx1, gy + 85, 4, 70, [0, 160, 230, 255]);
            draw_filled_rect(&mut img, cx2, gy + 85, 4, 70, [230, 0, 130, 255]);
        }
    }

    // 7. Bottom Footer Diagnostic Box
    draw_rect_outline(
        &mut img,
        90,
        2620,
        width - 180,
        190,
        3,
        [0, 0, 0, 255],
    );
    draw_text_bitmap(
        &mut img,
        120,
        2650,
        "DIAGNOSTICO DE CALIDAD DE IMPRESION - HP DESKJET 1115",
        3,
        [0, 0, 0, 255],
    );
    draw_text_bitmap(
        &mut img,
        120,
        2705,
        &format!(
            "CALIDAD: {} DPI  |  PAPEL: {}",
            cfg.quality.dpi(),
            cfg.paper_size.display_name()
        ),
        3,
        [60, 60, 60, 255],
    );
    draw_text_bitmap(
        &mut img,
        120,
        2755,
        "SI LAS LINEAS EN EL NUMERO 5 DE LOS GRUPOS A-D COINCIDEN, LOS CABEZALES ESTAN ALINEADOS.",
        3,
        [0, 114, 198, 255],
    );

    img
}

pub fn is_pdf_file(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_ascii_lowercase())
            .as_deref(),
        Some("pdf" | "ps" | "eps")
    )
}

fn gs_binary() -> &'static str {
    if Path::new("/opt/homebrew/bin/gs").exists() {
        "/opt/homebrew/bin/gs"
    } else {
        "gs"
    }
}

pub fn get_pdf_page_count(pdf_path: &Path) -> Result<u32, String> {
    if !pdf_path.exists() {
        return Err(format!("No existe el archivo '{}'", pdf_path.display()));
    }
    let escaped = pdf_path
        .to_string_lossy()
        .replace('\\', "\\\\")
        .replace('(', "\\(")
        .replace(')', "\\)");
    let cmd_str = format!("({escaped}) (r) file runpdfbegin pdfpagecount = quit");
    let out = Command::new(gs_binary())
        .args(["-q", "-dNODISPLAY", "-dNOSAFER", "-c", &cmd_str])
        .output()
        .map_err(|e| format!("No se pudo ejecutar Ghostscript para leer el PDF: {e}"))?;

    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("No se pudo leer el PDF: {}", stderr.trim()));
    }
    let count_str = String::from_utf8_lossy(&out.stdout).trim().to_string();
    count_str
        .lines()
        .last()
        .and_then(|l| l.trim().parse::<u32>().ok())
        .filter(|&n| n > 0)
        .ok_or_else(|| format!("No se pudo determinar la cantidad de páginas del PDF ('{count_str}')."))
}

pub fn parse_page_range(spec: &str, total_pages: u32) -> Result<Vec<u32>, String> {
    let trimmed = spec.trim();
    if trimmed.is_empty()
        || trimmed.eq_ignore_ascii_case("all")
        || trimmed.eq_ignore_ascii_case("todas")
    {
        return Ok((1..=total_pages).collect());
    }

    let mut result = Vec::new();
    for token in trimmed.split(',') {
        let part = token.trim();
        if part.is_empty() {
            continue;
        }
        if let Some((start_s, end_s)) = part.split_once('-') {
            let start = start_s
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Rango de páginas inválido: '{part}'"))?;
            let end = end_s
                .trim()
                .parse::<u32>()
                .map_err(|_| format!("Rango de páginas inválido: '{part}'"))?;
            if start == 0 || end == 0 || start > end || end > total_pages {
                return Err(format!(
                    "Rango '{part}' fuera de límites (el documento tiene {total_pages} páginas)."
                ));
            }
            for p in start..=end {
                if !result.contains(&p) {
                    result.push(p);
                }
            }
        } else {
            let p = part
                .parse::<u32>()
                .map_err(|_| format!("Número de página inválido: '{part}'"))?;
            if p == 0 || p > total_pages {
                return Err(format!(
                    "Página {p} fuera de límites (el documento tiene {total_pages} páginas)."
                ));
            }
            if !result.contains(&p) {
                result.push(p);
            }
        }
    }

    if result.is_empty() {
        return Err("No se especificó ninguna página válida.".to_string());
    }
    Ok(result)
}

pub fn gs_paper_args(paper_size: PaperSize) -> Vec<String> {
    match paper_size {
        PaperSize::A4 => vec!["-sPAPERSIZE=a4".to_string()],
        PaperSize::Letter => vec!["-sPAPERSIZE=letter".to_string()],
        PaperSize::Legal => vec!["-sPAPERSIZE=legal".to_string()],
        PaperSize::A5 => vec!["-sPAPERSIZE=a5".to_string()],
        PaperSize::Photo4x6 => vec![
            "-dDEVICEWIDTHPOINTS=288".to_string(),
            "-dDEVICEHEIGHTPOINTS=432".to_string(),
        ],
        PaperSize::Photo5x7 => vec![
            "-dDEVICEWIDTHPOINTS=360".to_string(),
            "-dDEVICEHEIGHTPOINTS=504".to_string(),
        ],
    }
}

fn append_chp2200_color_args(args: &mut Vec<String>, cfg: &PclJobConfig) {
    if cfg.color_mode.uses_cmy_black() {
        // Papertype=3 (ESC&l3M Glossy/Photo) directs HP dye+pigment firmware to render
        // blacks using composite CMY ink from the tricolor cartridge instead of pigment K.
        args.push("-dPapertype=3".to_string());
    }
    if cfg.color_mode.is_grayscale() {
        args.push("-sColorConversionStrategy=Gray".to_string());
    }
}

/// Encodes a single PDF page directly into verified HP DeskJet 1110/1115 PCL3GUI Mode 10 bytes
pub fn encode_pdf_page_to_pcl3gui_mode10(
    pdf_path: &Path,
    page_num: u32,
    cfg: &PclJobConfig,
) -> Result<Vec<u8>, String> {
    let mut args = vec![
        "-q".to_string(),
        "-dNOPAUSE".to_string(),
        "-dBATCH".to_string(),
        "-dSAFER".to_string(),
        format!("-dFirstPage={page_num}"),
        format!("-dLastPage={page_num}"),
        "-sDEVICE=chp2200".to_string(),
        format!("-r{}", cfg.quality.dpi()),
        "-dPDFFitPage".to_string(),
        "-dFIXEDMEDIA".to_string(),
    ];
    args.extend(gs_paper_args(cfg.paper_size));
    append_chp2200_color_args(&mut args, cfg);
    args.push("-sOutputFile=-".to_string());
    args.push(pdf_path.to_string_lossy().into_owned());

    let out = Command::new(gs_binary())
        .args(&args)
        .output()
        .map_err(|e| format!("Error ejecutando Ghostscript: {e}"))?;

    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!(
            "Error generando PCL3GUI para página {page_num}: {}",
            stderr.trim()
        ));
    }
    Ok(out.stdout)
}

/// Encodes an RGBA image into verified HP DeskJet 1110/1115 PCL3GUI Mode 10 bytes
pub fn encode_rgba_to_pcl3gui_mode10(
    img: &RgbaImage,
    cfg: &PclJobConfig,
) -> Result<Vec<u8>, String> {
    let (w_in, h_in) = cfg.paper_size.dimensions_inches();
    let page_w_pt = w_in * 72.0;
    let page_h_pt = h_in * 72.0;
    let margin_pt = 20.0;
    let avail_w = (page_w_pt - margin_pt * 2.0).max(72.0);
    let avail_h = (page_h_pt - margin_pt * 2.0).max(72.0);

    let iw = img.width() as f32;
    let ih = img.height() as f32;
    let scale = (avail_w / iw).min(avail_h / ih);
    let draw_w = iw * scale;
    let draw_h = ih * scale;
    let tx = (page_w_pt - draw_w) / 2.0;
    let ty = page_h_pt - margin_pt - draw_h;

    let mut args = vec![
        "-q".to_string(),
        "-dNOPAUSE".to_string(),
        "-dBATCH".to_string(),
        "-dSAFER".to_string(),
        "-sDEVICE=chp2200".to_string(),
        format!("-r{}", cfg.quality.dpi()),
        "-dFIXEDMEDIA".to_string(),
    ];
    args.extend(gs_paper_args(cfg.paper_size));
    append_chp2200_color_args(&mut args, cfg);
    args.push("-sOutputFile=-".to_string());
    args.push("-".to_string());

    let mut child = Command::new(gs_binary())
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("Error iniciando Ghostscript: {e}"))?;

    if let Some(mut stdin) = child.stdin.take() {
        let w = img.width();
        let h = img.height();
        let ps_header = format!(
            "%!PS-Adobe-3.0\n<< /PageSize [{page_w_pt:.2} {page_h_pt:.2}] >> setpagedevice\n\
             gsave\n{tx:.2} {ty:.2} translate\n{draw_w:.2} {draw_h:.2} scale\n\
             /DeviceRGB setcolorspace\n\
             << /ImageType 1 /Width {w} /Height {h} /BitsPerComponent 8 \
             /Decode [0 1 0 1 0 1] /ImageMatrix [{w} 0 0 -{h} 0 {h}] \
             /DataSource currentfile >> image\n"
        );
        let _ = stdin.write_all(ps_header.as_bytes());
        let raw = img.as_raw();
        let mut rgb_row = Vec::with_capacity((w as usize) * 3);
        for chunk in raw.chunks_exact((w as usize) * 4) {
            rgb_row.clear();
            for px in chunk.chunks_exact(4) {
                let (r, g, b) = read_rgb(px, 4);
                rgb_row.push(r);
                rgb_row.push(g);
                rgb_row.push(b);
            }
            if stdin.write_all(&rgb_row).is_err() {
                break;
            }
        }
        let _ = stdin.write_all(b"\ngrestore\nshowpage\n");
    }

    let out = child
        .wait_with_output()
        .map_err(|e| format!("Error esperando Ghostscript: {e}"))?;
    if !out.status.success() || out.stdout.is_empty() {
        let stderr = String::from_utf8_lossy(&out.stderr);
        return Err(format!("Error generando PCL3GUI: {}", stderr.trim()));
    }
    Ok(out.stdout)
}

pub fn render_single_pdf_page(
    pdf_path: &Path,
    page_num: u32,
    dpi: u16,
    paper_size: PaperSize,
) -> Result<RgbaImage, String> {
    let tmp_file = std::env::temp_dir().join(format!(
        "hp1115_p{}_{}.png",
        page_num,
        std::process::id()
    ));
    let _ = std::fs::remove_file(&tmp_file);

    let status = Command::new(gs_binary())
        .args([
            "-q",
            "-dNOPAUSE",
            "-dBATCH",
            "-dSAFER",
            &format!("-dFirstPage={page_num}"),
            &format!("-dLastPage={page_num}"),
            "-sDEVICE=png16m",
            &format!("-r{dpi}"),
            &format!("-sOutputFile={}", tmp_file.display()),
            &pdf_path.to_string_lossy(),
        ])
        .status()
        .map_err(|e| format!("Error ejecutando Ghostscript: {e}"))?;

    if !status.success() || !tmp_file.exists() {
        let _ = std::fs::remove_file(&tmp_file);
        return Err(format!(
            "No se pudo renderizar la página {page_num} de '{}'.",
            pdf_path.display()
        ));
    }

    let img = image::open(&tmp_file)
        .map_err(|e| format!("Error leyendo imagen de página {page_num}: {e}"))?;
    let _ = std::fs::remove_file(&tmp_file);
    Ok(fit_image_to_printable_area(img, dpi, paper_size))
}

pub fn fit_image_to_printable_area(
    img: DynamicImage,
    dpi: u16,
    paper_size: PaperSize,
) -> RgbaImage {
    let (w_in, h_in) = paper_size.dimensions_inches();
    let max_w = ((w_in - 0.28).max(3.5) * dpi as f32) as u32;
    let max_h = ((h_in - 0.68).max(4.5) * dpi as f32) as u32;

    let (iw, ih) = img.dimensions();
    if iw <= max_w && ih <= max_h {
        return img.to_rgba8();
    }

    let scale = (max_w as f32 / iw as f32).min(max_h as f32 / ih as f32);
    let nw = ((iw as f32 * scale) as u32).max(1);
    let nh = ((ih as f32 * scale) as u32).max(1);
    let resized = img
        .resize_exact(nw, nh, image::imageops::FilterType::Lanczos3)
        .to_rgba8();

    let mut canvas = RgbaImage::from_pixel(max_w, max_h, image::Rgba([255, 255, 255, 255]));
    let off_x = (max_w.saturating_sub(nw)) / 2;
    let off_y = (max_h.saturating_sub(nh)) / 2;
    image::imageops::overlay(&mut canvas, &resized, off_x as i64, off_y as i64);
    canvas
}

/// Converts a PDF or Image file on disk into an `RgbaImage` at the target DPI
pub fn load_file_as_page_images(path: &Path, dpi: u16) -> Result<Vec<RgbaImage>, String> {
    if is_pdf_file(path) {
        render_pdf_with_gs_or_sips(path, dpi)
    } else {
        let img = image::open(path)
            .map_err(|e| format!("No se pudo abrir la imagen '{}': {e}", path.display()))?;
        Ok(vec![fit_image_to_page(img, dpi)])
    }
}

fn fit_image_to_page(img: DynamicImage, dpi: u16) -> RgbaImage {
    let page_w = (8.0 * dpi as f32) as u32;
    let page_h = (10.5 * dpi as f32) as u32;
    let (iw, ih) = img.dimensions();
    let scale = (page_w as f32 / iw as f32)
        .min(page_h as f32 / ih as f32)
        .min(4.0);
    let nw = ((iw as f32 * scale) as u32).max(1);
    let nh = ((ih as f32 * scale) as u32).max(1);
    let resized = img
        .resize_exact(nw, nh, image::imageops::FilterType::Lanczos3)
        .to_rgba8();

    let mut canvas = RgbaImage::from_pixel(page_w, page_h, image::Rgba([255, 255, 255, 255]));
    let off_x = (page_w.saturating_sub(nw)) / 2;
    let off_y = 20;
    image::imageops::overlay(&mut canvas, &resized, off_x as i64, off_y as i64);
    canvas
}

fn render_pdf_with_gs_or_sips(pdf_path: &Path, dpi: u16) -> Result<Vec<RgbaImage>, String> {
    let tmp_dir = std::env::temp_dir().join(format!("hp1115_pdf_{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    let out_pattern = tmp_dir.join("page_%03d.png");

    let gs_bin = if Path::new("/opt/homebrew/bin/gs").exists() {
        "/opt/homebrew/bin/gs"
    } else {
        "gs"
    };

    let status = Command::new(gs_bin)
        .args([
            "-q",
            "-dNOPAUSE",
            "-dBATCH",
            "-dSAFER",
            "-sDEVICE=png16m",
            &format!("-r{dpi}"),
            &format!("-sOutputFile={}", out_pattern.display()),
            &pdf_path.to_string_lossy(),
        ])
        .status();

    let mut pages = Vec::new();
    if let Ok(st) = status {
        if st.success() {
            if let Ok(entries) = std::fs::read_dir(&tmp_dir) {
                let mut paths: Vec<_> = entries
                    .flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("png"))
                    .collect();
                paths.sort();
                for p in paths {
                    if let Ok(im) = image::open(&p) {
                        pages.push(im.to_rgba8());
                    }
                    let _ = std::fs::remove_file(&p);
                }
            }
        }
    }
    let _ = std::fs::remove_dir_all(&tmp_dir);

    if pages.is_empty() {
        return Err(
            "No se pudo rasterizar el documento PDF (verifica que el archivo PDF sea válido)."
                .to_string(),
        );
    }
    Ok(pages)
}

/// Streams a CUPS Raster v2/v3 input (`RaS2`/`2SaR`/`RaS3`/`3SaR`) into an HP DeskJet 1115 PCL3GUI/DW-PCL stream
pub fn convert_cups_raster_to_pcl<R: Read, W: Write>(
    mut reader: R,
    mut writer: W,
    cfg: &PclJobConfig,
) -> Result<usize, String> {
    let mut magic = [0u8; 4];
    reader
        .read_exact(&mut magic)
        .map_err(|e| format!("No se pudo leer cabecera CUPS Raster: {e}"))?;

    let (little_endian, version) = match &magic {
        b"RaS2" => (false, 2),
        b"2SaR" => (true, 2),
        b"RaS3" => (false, 3),
        b"3SaR" => (true, 3),
        _ => {
            return Err(format!(
                "Formato CUPS Raster desconocido: {:02x?}",
                magic
            ));
        }
    };

    let header_size = if version == 2 { 1796usize } else { 1796usize };
    let mut hdr = vec![0u8; header_size];
    let mut pages_printed = 0usize;
    let mut job_header_written = false;

    while reader.read_exact(&mut hdr).is_ok() {
        let read_u32 = |offset: usize| -> u32 {
            let b = [
                hdr[offset],
                hdr[offset + 1],
                hdr[offset + 2],
                hdr[offset + 3],
            ];
            if little_endian {
                u32::from_le_bytes(b)
            } else {
                u32::from_be_bytes(b)
            }
        };

        // Standard CUPS page header v2 offsets:
        // 276: HWResolution[0], 280: HWResolution[1]
        // 372: cupsWidth, 376: cupsHeight
        // 384: cupsBitsPerColor, 388: cupsBitsPerPixel, 392: cupsBytesPerLine
        let hw_res_x = read_u32(276).clamp(150, 600) as u16;
        let width = read_u32(372);
        let height = read_u32(376);
        let bits_per_pixel = read_u32(388);
        let bytes_per_line = read_u32(392) as usize;

        if width == 0 || height == 0 || bytes_per_line == 0 {
            break;
        }

        let mut page_cfg = cfg.clone();
        page_cfg.quality = match hw_res_x {
            0..=200 => PrintQuality::Draft150,
            201..=450 => PrintQuality::Normal300,
            _ => PrintQuality::High600,
        };
        if bits_per_pixel == 8 {
            page_cfg.color_mode = ColorMode::GrayscaleK;
        }

        let bpp = (bits_per_pixel / 8).clamp(1, 4) as usize;
        let total_bytes = (height as usize) * bytes_per_line;
        let mut raw_page = vec![0u8; total_bytes];

        if version == 3 {
            reader
                .read_exact(&mut raw_page)
                .map_err(|e| format!("Error leyendo página CUPS Raster v3: {e}"))?;
        } else {
            decode_cups_v2_rle(&mut reader, &mut raw_page, height as usize, bytes_per_line, bpp)?;
        }

        if page_cfg.use_pcl3gui_header {
            let mut rgba = RgbaImage::new(width, height);
            for y in 0..height {
                let row_off = (y as usize) * bytes_per_line;
                for x in 0..width {
                    let px = row_off + (x as usize) * bpp;
                    let (r, g, b) = read_rgb(&raw_page[px..], bpp);
                    rgba.put_pixel(x, y, image::Rgba([r, g, b, 255]));
                }
            }
            let pbuf = encode_rgba_to_pcl3gui_mode10(&rgba, &page_cfg)?;
            writer
                .write_all(&pbuf)
                .map_err(|e| format!("Error escribiendo página PCL3GUI: {e}"))?;
        } else {
            if !job_header_written {
                let mut hbuf = Vec::new();
                write_job_header(&mut hbuf, &page_cfg);
                writer
                    .write_all(&hbuf)
                    .map_err(|e| format!("Error escribiendo cabecera PCL: {e}"))?;
                job_header_written = true;
            }
            let mut pbuf = Vec::with_capacity(65536);
            encode_page_bitmap(&mut pbuf, width, height, &raw_page, bpp, &page_cfg);
            writer
                .write_all(&pbuf)
                .map_err(|e| format!("Error escribiendo página PCL: {e}"))?;
        }
        pages_printed += 1;
    }

    if job_header_written {
        let mut fbuf = Vec::new();
        write_job_footer(&mut fbuf);
        let _ = writer.write_all(&fbuf);
    }
    let _ = writer.flush();

    Ok(pages_printed)
}

fn decode_cups_v2_rle<R: Read>(
    reader: &mut R,
    out: &mut [u8],
    height: usize,
    bytes_per_line: usize,
    bpp: usize,
) -> Result<(), String> {
    let mut y = 0usize;
    let mut byte_buf = [0u8; 1];
    let mut line_buf = vec![0u8; bytes_per_line];

    while y < height {
        reader
            .read_exact(&mut byte_buf)
            .map_err(|e| format!("EOF en RLE vertical CUPS v2: {e}"))?;
        let line_repeat = (byte_buf[0] as usize) + 1;

        let mut x = 0usize;
        while x < bytes_per_line {
            reader
                .read_exact(&mut byte_buf)
                .map_err(|e| format!("EOF en RLE horizontal CUPS v2: {e}"))?;
            let count = byte_buf[0];
            if count <= 127 {
                // Repeat pixel (count + 1) times
                let reps = (count as usize) + 1;
                let mut pixel = [0u8; 4];
                reader
                    .read_exact(&mut pixel[..bpp])
                    .map_err(|e| format!("EOF leyendo pixel repetido CUPS v2: {e}"))?;
                for _ in 0..reps {
                    if x + bpp <= bytes_per_line {
                        line_buf[x..x + bpp].copy_from_slice(&pixel[..bpp]);
                        x += bpp;
                    } else {
                        break;
                    }
                }
            } else {
                // (257 - count) literal pixels
                let lit_pixels = 257 - (count as usize);
                let nbytes = (lit_pixels * bpp).min(bytes_per_line - x);
                reader
                    .read_exact(&mut line_buf[x..x + nbytes])
                    .map_err(|e| format!("EOF leyendo pixels literales CUPS v2: {e}"))?;
                x += nbytes;
            }
        }

        for _ in 0..line_repeat {
            if y < height {
                let dst = y * bytes_per_line;
                out[dst..dst + bytes_per_line].copy_from_slice(&line_buf);
                y += 1;
            }
        }
    }
    Ok(())
}

// --- Minimal built-in 5x7 bitmap font & drawing primitives for the Test Page ---

fn draw_filled_rect(img: &mut RgbaImage, x: u32, y: u32, w: u32, h: u32, color: [u8; 4]) {
    let max_x = (x + w).min(img.width());
    let max_y = (y + h).min(img.height());
    let px = image::Rgba(color);
    for py in y..max_y {
        for px_x in x..max_x {
            img.put_pixel(px_x, py, px);
        }
    }
}

fn draw_rect_outline(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    w: u32,
    h: u32,
    thickness: u32,
    color: [u8; 4],
) {
    draw_filled_rect(img, x, y, w, thickness, color);
    draw_filled_rect(img, x, y + h.saturating_sub(thickness), w, thickness, color);
    draw_filled_rect(img, x, y, thickness, h, color);
    draw_filled_rect(img, x + w.saturating_sub(thickness), y, thickness, h, color);
}

fn draw_text_bitmap(
    img: &mut RgbaImage,
    x: u32,
    y: u32,
    text: &str,
    scale: u32,
    color: [u8; 4],
) {
    let mut cx = x;
    for ch in text.chars() {
        let glyph = glyph_5x7(ch.to_ascii_uppercase());
        for (row_idx, row_bits) in glyph.iter().enumerate() {
            for col_idx in 0..5u32 {
                if (row_bits >> (4 - col_idx)) & 1 != 0 {
                    draw_filled_rect(
                        img,
                        cx + col_idx * scale,
                        y + (row_idx as u32) * scale,
                        scale,
                        scale,
                        color,
                    );
                }
            }
        }
        cx += 6 * scale;
    }
}

fn glyph_5x7(c: char) -> [u8; 7] {
    match c {
        'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'B' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10001, 0b10001, 0b11110],
        'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        'D' => [0b11100, 0b10010, 0b10001, 0b10001, 0b10001, 0b10010, 0b11100],
        'E' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b11111],
        'F' => [0b11111, 0b10000, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000],
        'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        'S' => [0b01111, 0b10000, 0b10000, 0b01110, 0b00001, 0b00001, 0b11110],
        'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b00100],
        'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        '0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        '1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        '2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        '3' => [0b11110, 0b00001, 0b00001, 0b01110, 0b00001, 0b00001, 0b11110],
        '4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        '5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        '6' => [0b00110, 0b01000, 0b10000, 0b11110, 0b10001, 0b10001, 0b01110],
        '7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        '8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        '9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00010, 0b01100],
        '-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        '+' => [0b00000, 0b00100, 0b00100, 0b11111, 0b00100, 0b00100, 0b00000],
        ':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        '.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b01100, 0b01100],
        ',' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100, 0b01000],
        '(' => [0b00010, 0b00100, 0b01000, 0b01000, 0b01000, 0b00100, 0b00010],
        ')' => [0b01000, 0b00100, 0b00010, 0b00010, 0b00010, 0b00100, 0b01000],
        '%' => [0b11001, 0b11010, 0b00010, 0b00100, 0b01000, 0b01011, 0b10011],
        '|' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        '/' => [0b00001, 0b00010, 0b00010, 0b00100, 0b01000, 0b01000, 0b10000],
        _ => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
    }
}
