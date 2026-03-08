/// Sixel image protocol decoder.
///
/// Sixel graphics are encoded as DCS sequences. Each sixel character encodes
/// a column of 6 vertical pixels. The format supports up to 256 colors defined
/// with `#N;2;R;G;B` (RGB percentages 0-100) or `#N;1;H;L;S` (HLS).
use std::collections::HashMap;

/// A decoded sixel image with RGBA pixel data in row-major order.
#[derive(Debug, Clone)]
pub struct SixelImage {
    pub width: u32,
    pub height: u32,
    /// RGBA bytes, row-major. Length = width * height * 4.
    pub pixels: Vec<u8>,
}

/// Maximum sixel data size (64 MiB). Sequences exceeding this are discarded
/// to prevent OOM from malicious or runaway terminal streams.
const MAX_SIXEL_DATA: usize = 64 * 1024 * 1024;

/// Maximum number of color registers in the sixel palette.
///
/// The sixel spec allows registers 0–65535, but real-world images rarely exceed
/// 256. We cap at 4096 to prevent unbounded memory growth from malicious
/// streams that define all 65536 registers.
const MAX_SIXEL_COLORS: usize = 4096;

/// Stateful DCS-hook decoder for sixel sequences.
///
/// Lives on `MarauderParser` across multiple VTE `hook` / `put` / `unhook` calls.
pub struct SixelDecoder {
    active: bool,
    overflow: bool,
    data: Vec<u8>,
}

impl SixelDecoder {
    pub fn new() -> Self {
        Self {
            active: false,
            overflow: false,
            data: Vec::new(),
        }
    }

    /// Called on the VTE DCS `hook` callback.
    ///
    /// Sixel sequences use the final byte `q` in the DCS introducer.
    /// Returns `true` when this decoder has claimed the DCS sequence.
    pub fn hook(&mut self, _params: &vte::Params, _intermediates: &[u8], action: char) -> bool {
        if action == 'q' {
            self.active = true;
            self.overflow = false;
            self.data.clear();
            true
        } else {
            false
        }
    }

    /// Called for every byte inside an active DCS sequence.
    ///
    /// Data beyond [`MAX_SIXEL_DATA`] is silently discarded and the sequence
    /// will produce no image on `unhook`.
    pub fn put(&mut self, byte: u8) {
        if self.active && !self.overflow {
            if self.data.len() >= MAX_SIXEL_DATA {
                self.overflow = true;
                self.data.clear();
                self.data.shrink_to_fit();
                return;
            }
            self.data.push(byte);
        }
    }

    /// Called on the VTE DCS `unhook` callback.
    ///
    /// Decodes and returns the image if the sequence was a valid sixel stream.
    pub fn unhook(&mut self) -> Option<SixelImage> {
        if !self.active {
            return None;
        }
        self.active = false;
        if self.overflow {
            self.overflow = false;
            return None;
        }
        let data = std::mem::take(&mut self.data);
        decode_sixel(&data)
    }

    pub fn is_active(&self) -> bool {
        self.active
    }

    pub fn is_overflow(&self) -> bool {
        self.overflow
    }
}

impl Default for SixelDecoder {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Internal decoding logic
// ---------------------------------------------------------------------------

/// RGBA color (each component 0-255).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Rgba(u8, u8, u8, u8);

impl Rgba {
    fn transparent() -> Self {
        Rgba(0, 0, 0, 0)
    }
}

/// Convert DEC HLS (hue 0-360, lightness 0-100, saturation 0-100) to RGB.
///
/// DEC VT340 uses a non-standard hue mapping: Blue=0°, Red=120°, Green=240°.
/// This differs from CSS/web HSL where Red=0°, Green=120°, Blue=240°.
/// We rotate the hue by +240° to convert DEC hue to standard HSL hue before
/// applying the standard algorithm.
fn hls_to_rgb(hue: f64, lightness: f64, saturation: f64) -> (u8, u8, u8) {
    let l = (lightness.clamp(0.0, 100.0)) / 100.0;
    let s = (saturation.clamp(0.0, 100.0)) / 100.0;

    if s == 0.0 {
        let v = (l * 255.0).round() as u8;
        return (v, v, v);
    }

    let q = if l < 0.5 {
        l * (1.0 + s)
    } else {
        l + s - l * s
    };
    let p = 2.0 * l - q;

    // Rotate DEC hue (Blue=0°) to standard hue (Red=0°): add 240°, mod 360°.
    let standard_hue = ((hue % 360.0) + 360.0 + 240.0) % 360.0;
    let hue_norm = standard_hue / 360.0;

    let channel = |t: f64| -> u8 {
        let t = if t < 0.0 {
            t + 1.0
        } else if t > 1.0 {
            t - 1.0
        } else {
            t
        };
        let v = if t < 1.0 / 6.0 {
            p + (q - p) * 6.0 * t
        } else if t < 1.0 / 2.0 {
            q
        } else if t < 2.0 / 3.0 {
            p + (q - p) * (2.0 / 3.0 - t) * 6.0
        } else {
            p
        };
        (v * 255.0).round() as u8
    };

    (
        channel(hue_norm + 1.0 / 3.0),
        channel(hue_norm),
        channel(hue_norm - 1.0 / 3.0),
    )
}

/// Core sixel stream decoder.
fn decode_sixel(data: &[u8]) -> Option<SixelImage> {
    // Strip the optional DCS parameter string that precedes the first sixel
    // character or color introducer.  The parameter string ends at the first
    // byte < 0x20 that is not part of the parameter grammar, but in practice
    // we skip everything up to the first `#` or sixel data byte.
    //
    // Sixel data bytes: 0x21 (`!`), 0x22 (raster attributes `"`), 0x23 (`#`),
    // 0x24 (`$`), 0x2D (`-`), 0x3F-0x7E (sixel pixels).

    let mut palette: HashMap<u32, Rgba> = HashMap::new();
    let mut current_color: u32 = 0;

    // Pixel buffer — grown as needed.  We don't know dimensions ahead of time.
    // We accumulate rows of 6-pixel bands; max_col tracks the widest row seen.
    let mut pixels: Vec<Vec<Rgba>> = Vec::new(); // indexed by y, then x
    let mut cursor_x: u32 = 0;
    // cursor_y is the top of the current sixel band (multiples of 6).
    let mut cursor_y: u32 = 0;
    // max_col: rightmost column index + 1 (counts cursor advancement, even for
    // zero-bitmap characters — every sixel char advances the cursor).
    let mut max_col: u32 = 0;
    // max_band_y: highest cursor_y value that was reached by any sixel character
    // (including zero-bitmap). Final image height = max_band_y + 6.
    let mut max_band_y: u32 = 0;
    let mut saw_any_sixel = false;

    let ensure_row = |pixels: &mut Vec<Vec<Rgba>>, y: u32| {
        let y = y as usize;
        while pixels.len() <= y {
            pixels.push(Vec::new());
        }
    };

    let set_pixel = |pixels: &mut Vec<Vec<Rgba>>, x: u32, y: u32, color: Rgba| {
        ensure_row(pixels, y);
        let row = &mut pixels[y as usize];
        let x = x as usize;
        if row.len() <= x {
            row.resize(x + 1, Rgba::transparent());
        }
        row[x] = color;
    };

    let mut i = 0;
    while i < data.len() {
        let byte = data[i];
        match byte {
            // Raster attributes `"Pa;Pb;Ph;Pv` — skip the parameter string.
            b'"' => {
                i += 1;
                while i < data.len() {
                    let b = data[i];
                    // Raster attribute parameters are digits and semicolons.
                    if b.is_ascii_digit() || b == b';' {
                        i += 1;
                    } else {
                        break;
                    }
                }
                // `i` now points at the byte after the attribute string.
                continue;
            }

            // Color introducer `#`
            b'#' => {
                i += 1;
                // Parse color index.
                let (color_idx, consumed) = parse_number(&data[i..]);
                i += consumed;

                if i < data.len() && data[i] == b';' {
                    // Color definition follows: #N;T;A;B;C
                    i += 1; // skip ';'
                    let (color_type, consumed) = parse_number(&data[i..]);
                    i += consumed;

                    // Skip ';'
                    if i < data.len() && data[i] == b';' {
                        i += 1;
                    }
                    let (a, consumed) = parse_number(&data[i..]);
                    i += consumed;
                    if i < data.len() && data[i] == b';' {
                        i += 1;
                    }
                    let (b_val, consumed) = parse_number(&data[i..]);
                    i += consumed;
                    if i < data.len() && data[i] == b';' {
                        i += 1;
                    }
                    let (c_val, consumed) = parse_number(&data[i..]);
                    i += consumed;

                    let rgba = match color_type {
                        2 => {
                            // RGB percentages 0-100
                            let r = ((a as f64 / 100.0) * 255.0).round() as u8;
                            let g = ((b_val as f64 / 100.0) * 255.0).round() as u8;
                            let b2 = ((c_val as f64 / 100.0) * 255.0).round() as u8;
                            Rgba(r, g, b2, 255)
                        }
                        1 => {
                            // HLS: hue 0-360, lightness 0-100, saturation 0-100
                            let (r, g, b2) =
                                hls_to_rgb(a as f64, b_val as f64, c_val as f64);
                            Rgba(r, g, b2, 255)
                        }
                        _ => Rgba(0, 0, 0, 255),
                    };
                    if palette.len() < MAX_SIXEL_COLORS || palette.contains_key(&color_idx) {
                        palette.insert(color_idx, rgba);
                    }
                    current_color = color_idx;
                } else {
                    // Just a color select, no definition.
                    current_color = color_idx;
                }
                continue;
            }

            // Repeat introducer `!`
            b'!' => {
                i += 1;
                let (repeat_count, consumed) = parse_number(&data[i..]);
                i += consumed;
                if i >= data.len() {
                    break;
                }
                let sixel_byte = data[i];
                i += 1;
                if sixel_byte < 0x3F || sixel_byte > 0x7E {
                    continue;
                }
                let bitmap = sixel_byte - 0x3F;
                let color = *palette
                    .get(&current_color)
                    .unwrap_or(&Rgba(255, 255, 255, 255));
                for _ in 0..repeat_count {
                    saw_any_sixel = true;
                    if max_col < cursor_x + 1 {
                        max_col = cursor_x + 1;
                    }
                    if max_band_y < cursor_y {
                        max_band_y = cursor_y;
                    }
                    for bit in 0..6u32 {
                        if (bitmap >> bit) & 1 == 1 {
                            set_pixel(&mut pixels, cursor_x, cursor_y + bit, color);
                        }
                    }
                    cursor_x += 1;
                }
                continue;
            }

            // Carriage return `$` — go to column 0 of current band.
            b'$' => {
                cursor_x = 0;
                i += 1;
                continue;
            }

            // Newline `-` — advance cursor_y by 6.
            b'-' => {
                cursor_y += 6;
                cursor_x = 0;
                i += 1;
                continue;
            }

            // Sixel data byte (0x3F..=0x7E).
            0x3F..=0x7E => {
                let bitmap = byte - 0x3F;
                let color = *palette
                    .get(&current_color)
                    .unwrap_or(&Rgba(255, 255, 255, 255));
                saw_any_sixel = true;
                if max_col < cursor_x + 1 {
                    max_col = cursor_x + 1;
                }
                if max_band_y < cursor_y {
                    max_band_y = cursor_y;
                }
                for bit in 0..6u32 {
                    if (bitmap >> bit) & 1 == 1 {
                        set_pixel(&mut pixels, cursor_x, cursor_y + bit, color);
                    }
                }
                cursor_x += 1;
                i += 1;
                continue;
            }

            // Skip control bytes and unknown bytes.
            _ => {
                i += 1;
                continue;
            }
        }
    }

    // We need at least one sixel character to produce an image.
    if !saw_any_sixel {
        return None;
    }

    // Height = highest band's top row + 6 (a full sixel band is always 6 pixels
    // tall regardless of how many bits are set in the final row's characters).
    let height = max_band_y + 6;
    let width = max_col;

    // Flatten into a contiguous RGBA byte vector.
    // The flat buffer is pre-zeroed (all transparent); we then paint in the
    // pixels that were actually set.  `pixels` may have fewer entries than
    // `height` if only the first few rows of the final band were set.
    let mut flat = vec![0u8; (width * height * 4) as usize];
    for (y, row) in pixels.iter().enumerate() {
        if y >= height as usize {
            break;
        }
        for x in 0..width as usize {
            let color = if x < row.len() {
                row[x]
            } else {
                Rgba::transparent()
            };
            let base = (y * width as usize + x) * 4;
            flat[base] = color.0;
            flat[base + 1] = color.1;
            flat[base + 2] = color.2;
            flat[base + 3] = color.3;
        }
    }

    Some(SixelImage {
        width,
        height,
        pixels: flat,
    })
}

/// Parse a decimal integer from the start of `data`.
///
/// Returns `(value, bytes_consumed)`. Consumes zero bytes and returns 0 if no
/// digit is found.
fn parse_number(data: &[u8]) -> (u32, usize) {
    let mut value: u32 = 0;
    let mut consumed = 0;
    for &b in data {
        if b.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add((b - b'0') as u32);
            consumed += 1;
        } else {
            break;
        }
    }
    (value, consumed)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // Helper: build a minimal sixel stream.
    // Defines color 0 = white RGB(100,100,100 %) then draws 1 column × 1 band.
    fn make_simple_sixel() -> Vec<u8> {
        // #0;2;100;100;100  — color 0 = white (100% R/G/B)
        // ?  — 0x3F = bitmap 0b000000 = no pixels (transparent column)
        // ~  — 0x7E = bitmap 0b111111 = all 6 pixels set
        let mut v: Vec<u8> = Vec::new();
        // Color definition
        v.extend_from_slice(b"#0;2;100;100;100");
        // One sixel char with all 6 bits set (byte 0x7E = '~')
        v.push(b'~');
        v
    }

    #[test]
    fn test_decode_single_column_all_pixels() {
        let data = make_simple_sixel();
        let img = decode_sixel(&data).expect("should decode");
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
        // All 6 rows in column 0 should be white+opaque.
        for row in 0..6usize {
            let base = row * 4;
            assert_eq!(img.pixels[base], 255, "R at row {row}");
            assert_eq!(img.pixels[base + 1], 255, "G at row {row}");
            assert_eq!(img.pixels[base + 2], 255, "B at row {row}");
            assert_eq!(img.pixels[base + 3], 255, "A at row {row}");
        }
    }

    #[test]
    fn test_decode_transparent_pixel() {
        // `?` = 0x3F = bitmap 0 — no pixels set, so color stays transparent.
        // Single color definition then `?` (no pixels set).
        let data = b"#0;2;100;0;0?".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        assert_eq!(img.width, 1);
        // The pixel at (0,0) should be transparent because no bits were set.
        let base = 0;
        assert_eq!(img.pixels[base + 3], 0, "pixel should be transparent");
    }

    #[test]
    fn test_decode_carriage_return_and_newline() {
        // Draw two columns in band 0, then CR+newline, then one column in band 1.
        // Color 0 = red; color 1 = green.
        // #0;2;100;0;0 #1;2;0;100;0
        // ~ ~    — two all-pixel columns (color 0, red)
        // $      — carriage return
        // #1     — select color 1
        // ~      — one all-pixel column at x=0 in band 0 (overwrite? no — sixel
        //          compositing paints only set bits, existing pixels from previous
        //          color remain if the new color has 0 for that bit — but here
        //          bitmap is all-1s so it overwrites with green)
        // -      — advance to band 1
        // ~      — one column in band 1
        let data = b"#0;2;100;0;0~~$#1;2;0;100;0~-~".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        // Width = 2 (two columns from the first run)
        assert_eq!(img.width, 2);
        // Height = 12 (2 bands of 6)
        assert_eq!(img.height, 12);
        // (0,0) should be green (color 1 overwrote color 0 at x=0 via $+#1+~)
        let base_0_0 = 0 * img.width as usize * 4;
        assert_eq!(img.pixels[base_0_0 + 1], 255, "G should be 255 at (0,0)");
        assert_eq!(img.pixels[base_0_0], 0, "R should be 0 at (0,0)");
        // (1,0) should be red (color 0, not overwritten)
        let base_0_1 = 1 * 4;
        assert_eq!(img.pixels[base_0_1], 255, "R should be 255 at (1,0)");
        assert_eq!(img.pixels[base_0_1 + 1], 0, "G should be 0 at (1,0)");
    }

    #[test]
    fn test_decode_repeat() {
        // !5~ = repeat `~` (all pixels) 5 times → 5 columns, all white.
        let data = b"#0;2;100;100;100!5~".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        assert_eq!(img.width, 5);
        assert_eq!(img.height, 6);
        for x in 0..5usize {
            let base = x * 4;
            assert_eq!(img.pixels[base], 255, "R at x={x}");
            assert_eq!(img.pixels[base + 3], 255, "A at x={x}");
        }
    }

    #[test]
    fn test_decode_hls_color_hue0_is_blue() {
        // DEC HLS: hue=0 → Blue (not Red as in CSS HSL).
        let data = b"#0;1;0;50;100~".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        let (r, g, b) = (img.pixels[0], img.pixels[1], img.pixels[2]);
        assert!(b > 200, "expected blue-dominant, got R={r} G={g} B={b}");
        assert!(r < 10, "expected low red, got R={r}");
        assert!(g < 10, "expected low green, got G={g}");
    }

    #[test]
    fn test_decode_hls_color_hue120_is_red() {
        // DEC HLS: hue=120 → Red.
        let data = b"#0;1;120;50;100~".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        let (r, g, b) = (img.pixels[0], img.pixels[1], img.pixels[2]);
        assert!(r > 200, "expected red-dominant, got R={r} G={g} B={b}");
        assert!(g < 10, "expected low green, got G={g}");
        assert!(b < 10, "expected low blue, got B={b}");
    }

    #[test]
    fn test_decode_hls_color_hue240_is_green() {
        // DEC HLS: hue=240 → Green.
        let data = b"#0;1;240;50;100~".to_vec();
        let img = decode_sixel(&data).expect("should decode");
        let (r, g, b) = (img.pixels[0], img.pixels[1], img.pixels[2]);
        assert!(g > 200, "expected green-dominant, got R={r} G={g} B={b}");
        assert!(r < 10, "expected low red, got R={r}");
        assert!(b < 10, "expected low blue, got B={b}");
    }

    #[test]
    fn test_hls_saturation_zero_is_gray() {
        // saturation=0 → achromatic, lightness determines gray level.
        let (r, g, b) = hls_to_rgb(0.0, 50.0, 0.0);
        assert_eq!(r, g);
        assert_eq!(g, b);
        assert_eq!(r, 128); // 50% lightness → 128
    }

    #[test]
    fn test_hls_lightness_zero_is_black() {
        let (r, g, b) = hls_to_rgb(120.0, 0.0, 100.0);
        assert_eq!((r, g, b), (0, 0, 0));
    }

    #[test]
    fn test_hls_lightness_100_is_white() {
        let (r, g, b) = hls_to_rgb(120.0, 100.0, 100.0);
        assert_eq!((r, g, b), (255, 255, 255));
    }

    #[test]
    fn test_hls_hue_wraps_at_360() {
        // hue=360 should be equivalent to hue=0 (both Blue in DEC).
        let a = hls_to_rgb(0.0, 50.0, 100.0);
        let b = hls_to_rgb(360.0, 50.0, 100.0);
        assert_eq!(a, b);
    }

    #[test]
    fn test_sixel_decoder_hook_put_unhook() {
        let mut decoder = SixelDecoder::new();
        // Simulate VTE DCS hook with action 'q'
        let params = vte::Params::default();
        let claimed = decoder.hook(&params, &[], 'q');
        assert!(claimed);
        assert!(decoder.is_active());
        // Feed the sixel bytes via put()
        for b in b"#0;2;100;100;100~" {
            decoder.put(*b);
        }
        // Unhook should return the image.
        let result = decoder.unhook();
        assert!(!decoder.is_active());
        let img = result.expect("should produce image");
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn test_sixel_decoder_non_sixel_dcs() {
        let mut decoder = SixelDecoder::new();
        let params = vte::Params::default();
        // Non-sixel DCS (e.g., DECRQSS uses 'q' too? Let's use 'p' as a different byte)
        let claimed = decoder.hook(&params, &[], 'p');
        assert!(!claimed);
        assert!(!decoder.is_active());
        let result = decoder.unhook();
        assert!(result.is_none());
    }

    #[test]
    fn test_parse_number_helper() {
        assert_eq!(parse_number(b"123abc"), (123, 3));
        assert_eq!(parse_number(b"abc"), (0, 0));
        assert_eq!(parse_number(b"0"), (0, 1));
    }

    #[test]
    fn test_empty_data_returns_none() {
        assert!(decode_sixel(b"").is_none());
    }

    #[test]
    fn test_only_color_def_no_data_returns_none() {
        // A color definition with no sixel pixel data → width stays 0.
        assert!(decode_sixel(b"#0;2;100;0;0").is_none());
    }

    #[test]
    fn test_palette_capped_at_max_colors() {
        // Define MAX_SIXEL_COLORS + 100 unique colors, then verify palette is capped.
        let mut data: Vec<u8> = Vec::new();
        for i in 0..(MAX_SIXEL_COLORS + 100) {
            // #N;2;R;G;B where R = i % 100
            data.extend_from_slice(format!("#{};2;{};0;0", i, i % 100).as_bytes());
        }
        // Add one sixel character so the image is produced.
        data.push(b'~');
        let img = decode_sixel(&data);
        assert!(img.is_some(), "should still decode");
        // We can't directly inspect palette size from outside, but we verify
        // the decoder doesn't OOM and produces a valid image.
        let img = img.unwrap();
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
    }

    #[test]
    fn test_palette_allows_redefine_existing_color() {
        // Redefining an already-registered color should always succeed,
        // even at the cap.
        let mut data: Vec<u8> = Vec::new();
        // Fill palette to MAX_SIXEL_COLORS with unique indices.
        for i in 0..MAX_SIXEL_COLORS {
            data.extend_from_slice(format!("#{};2;50;50;50", i).as_bytes());
        }
        // Redefine color 0 (already exists) — should succeed.
        data.extend_from_slice(b"#0;2;100;0;0");
        data.push(b'~');
        let img = decode_sixel(&data).expect("should decode");
        // Color 0 should be red (the redefinition).
        assert_eq!(img.pixels[0], 255, "R should be 255 after redefine");
        assert_eq!(img.pixels[1], 0, "G should be 0 after redefine");
    }

    #[test]
    fn test_bitmap_partial_bits() {
        // `@` = 0x40 = bitmap 0b000001 = only bit 0 set → pixel at y=cursor_y+0.
        // `A` = 0x41 = bitmap 0b000010 = only bit 1 set → pixel at y=cursor_y+1.
        let data = b"#0;2;100;0;0@".to_vec(); // 0x40 - 0x3F = 1 = 0b000001
        let img = decode_sixel(&data).expect("should decode");
        assert_eq!(img.width, 1);
        assert_eq!(img.height, 6);
        // Row 0 should be red (bit 0 set).
        assert_eq!(img.pixels[0], 255, "R row 0");
        assert_eq!(img.pixels[3], 255, "A row 0");
        // Row 1 should be transparent (bit 1 not set).
        assert_eq!(img.pixels[4 + 3], 0, "A row 1 should be transparent");
    }
}
