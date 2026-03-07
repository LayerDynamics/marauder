//! Glyph atlas: rasterizes glyphs via cosmic-text, packs into GPU texture.

use std::collections::HashMap;
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache};
use tracing;

/// Glyph UV coordinates in the atlas texture.
#[derive(Debug, Clone, Copy)]
pub struct GlyphEntry {
    /// UV rect: (u, v, width, height) in normalized [0,1] coordinates.
    pub uv: [f32; 4],
    /// Glyph pixel size.
    pub pixel_size: [f32; 2],
    /// Bearing offset from cell origin.
    pub offset: [f32; 2],
}

/// Atlas texture dimensions.
const ATLAS_SIZE: u32 = 1024;

/// Manages glyph rasterization and GPU texture atlas.
pub struct GlyphAtlas {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Cache: char → atlas entry.
    entries: HashMap<char, GlyphEntry>,
    /// Raw RGBA pixel data for the atlas texture.
    pixels: Vec<u8>,
    /// Current packing cursor (simple row-based packing).
    pack_x: u32,
    pack_y: u32,
    row_height: u32,
    /// Font metrics.
    cell_width: f32,
    cell_height: f32,
    font_size: f32,
    /// Font family name (e.g. "JetBrains Mono", "monospace").
    font_family: String,
    /// Font ascent in pixels (distance from baseline to top of cell).
    ascent: f32,
    /// Whether the atlas texture needs re-upload.
    dirty: bool,
}

impl GlyphAtlas {
    /// Create a new atlas with the given font configuration.
    pub fn new(font_family: &str, font_size: f32, line_height: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();
        let pixels = vec![0u8; (ATLAS_SIZE * ATLAS_SIZE) as usize];

        let cell_height = (font_size * line_height).ceil();
        let cell_width = (font_size * 0.6).ceil(); // Monospace approximation

        let family = Self::parse_family(font_family);

        // Derive ascent from font metrics using a probe buffer
        let metrics = Metrics::new(font_size, cell_height);
        let mut probe = Buffer::new(&mut font_system, metrics);
        let attrs = Attrs::new().family(family);
        probe.set_text(&mut font_system, "M", attrs, Shaping::Advanced);
        probe.shape_until_scroll(&mut font_system, false);
        let ascent = probe.layout_runs()
            .next()
            .map(|run| run.line_y)
            .unwrap_or(cell_height * 0.8);

        Self {
            font_system,
            swash_cache,
            entries: HashMap::new(),
            pixels,
            pack_x: 0,
            pack_y: 0,
            row_height: 0,
            cell_width,
            cell_height,
            font_size,
            font_family: font_family.to_string(),
            ascent,
            dirty: true,
        }
    }

    /// Parse a family name string into a cosmic-text `Family` variant.
    fn parse_family(name: &str) -> Family<'_> {
        match name.to_lowercase().as_str() {
            "monospace" | "" => Family::Monospace,
            "serif" => Family::Serif,
            "sans-serif" | "sans serif" => Family::SansSerif,
            "cursive" => Family::Cursive,
            "fantasy" => Family::Fantasy,
            _ => Family::Name(name),
        }
    }

    /// Get the cell dimensions.
    pub fn cell_size(&self) -> (f32, f32) {
        (self.cell_width, self.cell_height)
    }

    /// Get the font ascent in pixels (baseline position from top of cell).
    pub fn ascent(&self) -> f32 {
        self.ascent
    }

    /// Atlas texture size.
    pub fn atlas_size(&self) -> u32 {
        ATLAS_SIZE
    }

    /// Whether the atlas texture has been modified since last upload.
    pub fn is_dirty(&self) -> bool {
        self.dirty
    }

    /// Mark atlas as uploaded.
    pub fn clear_dirty(&mut self) {
        self.dirty = false;
    }

    /// Get the raw pixel data for uploading to GPU.
    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    /// Look up or rasterize a glyph, returning its atlas entry.
    pub fn get_or_insert(&mut self, c: char) -> Option<GlyphEntry> {
        if let Some(entry) = self.entries.get(&c) {
            return Some(*entry);
        }

        // Skip control characters and spaces (no visible glyph)
        if c.is_control() || c == ' ' {
            return None;
        }

        self.rasterize(c)
    }

    /// Rasterize a single character and pack into the atlas.
    fn rasterize(&mut self, c: char) -> Option<GlyphEntry> {
        let metrics = Metrics::new(self.font_size, self.cell_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);

        let family = Self::parse_family(&self.font_family);
        let attrs = Attrs::new().family(family);
        buffer.set_text(&mut self.font_system, &c.to_string(), attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Extract the glyph image from the first layout run
        let mut glyph_width = 0u32;
        let mut glyph_height = 0u32;
        let mut glyph_pixels: Vec<u8> = Vec::new();
        let mut glyph_left = 0i32;
        let mut glyph_top = 0i32;

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                if let Some(image) = self.swash_cache.get_image(&mut self.font_system, physical.cache_key) {
                    glyph_width = image.placement.width;
                    glyph_height = image.placement.height;
                    glyph_left = image.placement.left;
                    glyph_top = image.placement.top;

                    // Convert to single-channel alpha
                    match image.content {
                        cosmic_text::SwashContent::Mask => {
                            glyph_pixels = image.data.clone();
                        }
                        cosmic_text::SwashContent::Color => {
                            // Extract alpha channel from RGBA
                            glyph_pixels = image.data.chunks(4)
                                .map(|px| px.get(3).copied().unwrap_or(0))
                                .collect();
                        }
                        cosmic_text::SwashContent::SubpixelMask => {
                            // Average the RGB subpixel channels
                            glyph_pixels = image.data.chunks(3)
                                .map(|px| {
                                    let r = px.first().copied().unwrap_or(0) as u16;
                                    let g = px.get(1).copied().unwrap_or(0) as u16;
                                    let b = px.get(2).copied().unwrap_or(0) as u16;
                                    ((r + g + b) / 3) as u8
                                })
                                .collect();
                        }
                    }
                    break; // Take first glyph only
                }
            }
            break; // Take first run only
        }

        if glyph_width == 0 || glyph_height == 0 {
            // No renderable glyph — cache as a miss but return None
            tracing::trace!(char = ?c, "No glyph image produced");
            return None;
        }

        // Pack into atlas (simple left-to-right, top-to-bottom)
        if self.pack_x + glyph_width > ATLAS_SIZE {
            self.pack_x = 0;
            self.pack_y += self.row_height;
            self.row_height = 0;
        }
        if self.pack_y + glyph_height > ATLAS_SIZE {
            tracing::warn!("Glyph atlas full, cannot pack glyph for '{}'", c);
            return None;
        }

        // Copy glyph pixels into atlas
        for row in 0..glyph_height {
            let src_offset = (row * glyph_width) as usize;
            let dst_offset = ((self.pack_y + row) * ATLAS_SIZE + self.pack_x) as usize;
            let width = glyph_width as usize;
            if src_offset + width <= glyph_pixels.len()
                && dst_offset + width <= self.pixels.len()
            {
                self.pixels[dst_offset..dst_offset + width]
                    .copy_from_slice(&glyph_pixels[src_offset..src_offset + width]);
            }
        }

        let atlas_size_f = ATLAS_SIZE as f32;
        let entry = GlyphEntry {
            uv: [
                self.pack_x as f32 / atlas_size_f,
                self.pack_y as f32 / atlas_size_f,
                glyph_width as f32 / atlas_size_f,
                glyph_height as f32 / atlas_size_f,
            ],
            pixel_size: [glyph_width as f32, glyph_height as f32],
            offset: [glyph_left as f32, -glyph_top as f32],
        };

        self.pack_x += glyph_width + 1; // +1 padding
        self.row_height = self.row_height.max(glyph_height + 1);
        self.entries.insert(c, entry);
        self.dirty = true;

        Some(entry)
    }

    /// Look up or rasterize a ligature (multi-character glyph sequence).
    ///
    /// Uses cosmic-text shaping to detect ligatures. Returns `None` if the font
    /// does not provide a ligature for the given character sequence, or if the
    /// sequence is empty.
    pub fn get_or_insert_ligature(&mut self, chars: &[char]) -> Option<GlyphEntry> {
        if chars.is_empty() {
            return None;
        }
        // Single chars delegate to the standard path
        if chars.len() == 1 {
            return self.get_or_insert(chars[0]);
        }

        // Build a combined string key for cache lookup
        let key_str: String = chars.iter().collect();

        // For now, rasterize the combined string as a single glyph via cosmic-text shaping.
        // cosmic-text's Advanced shaping will produce ligature glyphs if the font supports them.
        let metrics = Metrics::new(self.font_size, self.cell_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let family = Self::parse_family(&self.font_family);
        let attrs = Attrs::new().family(family);
        buffer.set_text(&mut self.font_system, &key_str, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);

        // Check if shaping produced fewer glyphs than chars (i.e., a ligature was applied)
        let mut glyph_count = 0usize;
        for run in buffer.layout_runs() {
            glyph_count += run.glyphs.len();
            break; // first run only
        }

        if glyph_count == 0 || glyph_count >= chars.len() {
            // No ligature detected — font rendered each char individually
            return None;
        }

        // Ligature detected — rasterize the shaped ligature glyph from the buffer
        tracing::trace!(ligature = %key_str, glyphs = glyph_count, "Ligature detected");

        let mut glyph_width = 0u32;
        let mut glyph_height = 0u32;
        let mut glyph_pixels: Vec<u8> = Vec::new();
        let mut glyph_left = 0i32;
        let mut glyph_top = 0i32;

        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                if let Some(image) = self.swash_cache.get_image(&mut self.font_system, physical.cache_key) {
                    glyph_width = image.placement.width;
                    glyph_height = image.placement.height;
                    glyph_left = image.placement.left;
                    glyph_top = image.placement.top;

                    match image.content {
                        cosmic_text::SwashContent::Mask => {
                            glyph_pixels = image.data.clone();
                        }
                        cosmic_text::SwashContent::Color => {
                            glyph_pixels = image.data.chunks(4)
                                .map(|px| px.get(3).copied().unwrap_or(0))
                                .collect();
                        }
                        cosmic_text::SwashContent::SubpixelMask => {
                            glyph_pixels = image.data.chunks(3)
                                .map(|px| {
                                    let r = px.first().copied().unwrap_or(0) as u16;
                                    let g = px.get(1).copied().unwrap_or(0) as u16;
                                    let b = px.get(2).copied().unwrap_or(0) as u16;
                                    ((r + g + b) / 3) as u8
                                })
                                .collect();
                        }
                    }
                    break; // Take the ligature glyph
                }
            }
            break; // First run only
        }

        if glyph_width == 0 || glyph_height == 0 {
            return None;
        }

        // Pack into atlas
        if self.pack_x + glyph_width > ATLAS_SIZE {
            self.pack_x = 0;
            self.pack_y += self.row_height;
            self.row_height = 0;
        }
        if self.pack_y + glyph_height > ATLAS_SIZE {
            tracing::warn!("Glyph atlas full, cannot pack ligature '{}'", key_str);
            return None;
        }

        for row in 0..glyph_height {
            let src_offset = (row * glyph_width) as usize;
            let dst_offset = ((self.pack_y + row) * ATLAS_SIZE + self.pack_x) as usize;
            let width = glyph_width as usize;
            if src_offset + width <= glyph_pixels.len()
                && dst_offset + width <= self.pixels.len()
            {
                self.pixels[dst_offset..dst_offset + width]
                    .copy_from_slice(&glyph_pixels[src_offset..src_offset + width]);
            }
        }

        let atlas_size_f = ATLAS_SIZE as f32;
        let entry = GlyphEntry {
            uv: [
                self.pack_x as f32 / atlas_size_f,
                self.pack_y as f32 / atlas_size_f,
                glyph_width as f32 / atlas_size_f,
                glyph_height as f32 / atlas_size_f,
            ],
            pixel_size: [glyph_width as f32, glyph_height as f32],
            offset: [glyph_left as f32, -glyph_top as f32],
        };

        self.pack_x += glyph_width + 1;
        self.row_height = self.row_height.max(glyph_height + 1);
        // Cache under the first char — ligature lookups will re-detect via shaping
        self.entries.insert(chars[0], entry);
        self.dirty = true;

        Some(entry)
    }

    /// Pre-warm the atlas with ASCII printable characters.
    pub fn prewarm_ascii(&mut self) {
        for c in 0x20u8..=0x7Eu8 {
            self.get_or_insert(c as char);
        }
        tracing::debug!(
            glyphs = self.entries.len(),
            "Atlas pre-warmed with ASCII glyphs"
        );
    }
}
