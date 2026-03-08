//! Glyph atlas: rasterizes glyphs via cosmic-text, packs into GPU textures.
//!
//! Uses multi-page atlas overflow: when a 1024×1024 page fills up, a new page
//! is allocated automatically. Each `GlyphEntry` records which page it lives on
//! so the renderer can select the correct texture for drawing.

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
    /// Atlas page index (0 = primary page, 1+ = overflow pages).
    pub page: u16,
}

/// Atlas texture dimensions (per page).
const ATLAS_SIZE: u32 = 1024;

/// Maximum number of atlas pages to prevent unbounded memory growth.
const MAX_ATLAS_PAGES: usize = 16;

/// One page of atlas pixel data with its own packing cursor.
struct AtlasPage {
    /// Raw pixel data for this page.
    pixels: Vec<u8>,
    /// Current packing cursor X.
    pack_x: u32,
    /// Current packing cursor Y.
    pack_y: u32,
    /// Current row height in the packing cursor.
    row_height: u32,
    /// Whether this page has been modified since last GPU upload.
    dirty: bool,
    /// Bytes per pixel (1 for R8, 4 for RGBA).
    bpp: u32,
}

impl AtlasPage {
    fn new(bpp: u32) -> Self {
        Self {
            pixels: vec![0u8; (ATLAS_SIZE * ATLAS_SIZE * bpp) as usize],
            pack_x: 0,
            pack_y: 0,
            row_height: 0,
            dirty: true,
            bpp,
        }
    }

    /// Try to pack a glyph into this page. Returns `None` if it doesn't fit.
    fn try_pack(&mut self, glyph: &RawGlyphImage) -> Option<GlyphEntry> {
        if self.pack_x + glyph.width > ATLAS_SIZE {
            self.pack_x = 0;
            self.pack_y += self.row_height;
            self.row_height = 0;
        }
        if self.pack_y + glyph.height > ATLAS_SIZE {
            return None; // Page full
        }

        let row_bytes = (glyph.width * self.bpp) as usize;
        let atlas_row_bytes = (ATLAS_SIZE * self.bpp) as usize;
        for row in 0..glyph.height {
            let src_offset = (row * glyph.width * self.bpp) as usize;
            let dst_offset = ((self.pack_y + row) as usize) * atlas_row_bytes
                + (self.pack_x * self.bpp) as usize;
            if src_offset + row_bytes <= glyph.pixels.len()
                && dst_offset + row_bytes <= self.pixels.len()
            {
                self.pixels[dst_offset..dst_offset + row_bytes]
                    .copy_from_slice(&glyph.pixels[src_offset..src_offset + row_bytes]);
            }
        }

        let atlas_size_f = ATLAS_SIZE as f32;
        let entry = GlyphEntry {
            uv: [
                self.pack_x as f32 / atlas_size_f,
                self.pack_y as f32 / atlas_size_f,
                glyph.width as f32 / atlas_size_f,
                glyph.height as f32 / atlas_size_f,
            ],
            pixel_size: [glyph.width as f32, glyph.height as f32],
            offset: [glyph.left as f32, -glyph.top as f32],
            page: 0, // Caller sets this after knowing which page was used
        };

        self.pack_x += glyph.width + 1; // +1 padding
        self.row_height = self.row_height.max(glyph.height + 1);
        self.dirty = true;

        Some(entry)
    }
}

/// Raw glyph image extracted from cosmic-text shaping.
struct RawGlyphImage {
    width: u32,
    height: u32,
    left: i32,
    top: i32,
    pixels: Vec<u8>,
}

/// Manages glyph rasterization and GPU texture atlas with multi-page overflow.
pub struct GlyphAtlas {
    font_system: FontSystem,
    swash_cache: SwashCache,
    /// Cache: char → atlas entry.
    entries: HashMap<char, GlyphEntry>,
    /// R8 atlas pages (page 0 is primary, additional pages on overflow).
    pages: Vec<AtlasPage>,
    /// Font metrics.
    cell_width: f32,
    cell_height: f32,
    font_size: f32,
    /// Font family name (e.g. "JetBrains Mono", "monospace").
    font_family: String,
    /// Font ascent in pixels (distance from baseline to top of cell).
    ascent: f32,
    /// Cache: multi-char string → ligature atlas entry.
    ligature_entries: HashMap<String, GlyphEntry>,
    /// Subpixel RGBA atlas pages.
    subpixel_pages: Vec<AtlasPage>,
    /// Subpixel glyph entries (separate from standard entries).
    subpixel_entries: HashMap<char, GlyphEntry>,
}

impl GlyphAtlas {
    /// Create a new atlas with the given font configuration.
    pub fn new(font_family: &str, font_size: f32, line_height: f32) -> Self {
        let mut font_system = FontSystem::new();
        let swash_cache = SwashCache::new();

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
            ligature_entries: HashMap::new(),
            pages: vec![AtlasPage::new(1)],
            cell_width,
            cell_height,
            font_size,
            font_family: font_family.to_string(),
            ascent,
            subpixel_pages: vec![AtlasPage::new(4)],
            subpixel_entries: HashMap::new(),
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

    /// Atlas texture size (per page).
    pub fn atlas_size(&self) -> u32 {
        ATLAS_SIZE
    }

    /// Current vertical packing cursor position (page 0, for backward compat).
    pub fn pack_y(&self) -> u32 {
        self.pages[0].pack_y
    }

    /// Current row height in the packing cursor (page 0, for backward compat).
    pub fn row_height(&self) -> u32 {
        self.pages[0].row_height
    }

    /// Whether any R8 atlas page has been modified since last upload.
    pub fn is_dirty(&self) -> bool {
        self.pages.iter().any(|p| p.dirty)
    }

    /// Mark all R8 atlas pages as uploaded.
    pub fn clear_dirty(&mut self) {
        for page in &mut self.pages {
            page.dirty = false;
        }
    }

    /// Get the raw pixel data for page 0 (backward compatible).
    pub fn pixels(&self) -> &[u8] {
        &self.pages[0].pixels
    }

    /// Number of R8 atlas pages.
    pub fn page_count(&self) -> usize {
        self.pages.len()
    }

    /// Get the pixel data for a specific R8 atlas page.
    pub fn page_pixels(&self, page: usize) -> Option<&[u8]> {
        self.pages.get(page).map(|p| p.pixels.as_slice())
    }

    /// Get indices of R8 pages that need re-upload.
    pub fn dirty_pages(&self) -> Vec<usize> {
        self.pages.iter().enumerate()
            .filter(|(_, p)| p.dirty)
            .map(|(i, _)| i)
            .collect()
    }

    /// Mark a specific R8 page as uploaded.
    pub fn clear_page_dirty(&mut self, page: usize) {
        if let Some(p) = self.pages.get_mut(page) {
            p.dirty = false;
        }
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

    /// Shape text and extract the first glyph image as single-channel (R8) alpha.
    fn shape_and_extract_r8(&mut self, text: &str) -> Option<RawGlyphImage> {
        let buffer = self.shape_text(text);
        self.extract_first_glyph(&buffer, Self::convert_to_r8)
    }

    /// Shape text and extract the first glyph image as RGBA for subpixel rendering.
    fn shape_and_extract_rgba(&mut self, text: &str) -> Option<RawGlyphImage> {
        let buffer = self.shape_text(text);
        self.extract_first_glyph(&buffer, Self::convert_to_rgba)
    }

    /// Set up a cosmic-text buffer, shape the given text, and return it.
    fn shape_text(&mut self, text: &str) -> Buffer {
        let metrics = Metrics::new(self.font_size, self.cell_height);
        let mut buffer = Buffer::new(&mut self.font_system, metrics);
        let family = Self::parse_family(&self.font_family);
        let attrs = Attrs::new().family(family);
        buffer.set_text(&mut self.font_system, text, attrs, Shaping::Advanced);
        buffer.shape_until_scroll(&mut self.font_system, false);
        buffer
    }

    /// Extract the first glyph from a shaped buffer, converting pixels with `convert`.
    fn extract_first_glyph(
        &mut self,
        buffer: &Buffer,
        convert: fn(&cosmic_text::SwashImage) -> Vec<u8>,
    ) -> Option<RawGlyphImage> {
        for run in buffer.layout_runs() {
            for glyph in run.glyphs.iter() {
                let physical = glyph.physical((0.0, 0.0), 1.0);
                if let Some(image) = self.swash_cache.get_image(&mut self.font_system, physical.cache_key) {
                    let width = image.placement.width;
                    let height = image.placement.height;
                    if width == 0 || height == 0 {
                        return None;
                    }
                    return Some(RawGlyphImage {
                        width,
                        height,
                        left: image.placement.left,
                        top: image.placement.top,
                        pixels: convert(image),
                    });
                }
            }
            break; // first run only
        }
        None
    }

    /// Convert a swash image to single-channel alpha (R8).
    fn convert_to_r8(image: &cosmic_text::SwashImage) -> Vec<u8> {
        match image.content {
            cosmic_text::SwashContent::Mask => image.data.clone(),
            cosmic_text::SwashContent::Color => {
                image.data.chunks(4)
                    .map(|px| px.get(3).copied().unwrap_or(0))
                    .collect()
            }
            cosmic_text::SwashContent::SubpixelMask => {
                image.data.chunks(3)
                    .map(|px| {
                        let r = px.first().copied().unwrap_or(0) as u16;
                        let g = px.get(1).copied().unwrap_or(0) as u16;
                        let b = px.get(2).copied().unwrap_or(0) as u16;
                        ((r + g + b) / 3) as u8
                    })
                    .collect()
            }
        }
    }

    /// Convert a swash image to RGBA for subpixel rendering.
    fn convert_to_rgba(image: &cosmic_text::SwashImage) -> Vec<u8> {
        match image.content {
            cosmic_text::SwashContent::SubpixelMask => {
                let mut rgba = Vec::with_capacity(image.data.len() / 3 * 4);
                for px in image.data.chunks(3) {
                    let r = px.first().copied().unwrap_or(0);
                    let g = px.get(1).copied().unwrap_or(0);
                    let b = px.get(2).copied().unwrap_or(0);
                    rgba.extend_from_slice(&[r, g, b, r.max(g).max(b)]);
                }
                rgba
            }
            cosmic_text::SwashContent::Mask => {
                let mut rgba = Vec::with_capacity(image.data.len() * 4);
                for &a in &image.data {
                    rgba.extend_from_slice(&[a, a, a, a]);
                }
                rgba
            }
            cosmic_text::SwashContent::Color => image.data.clone(),
        }
    }

    /// Pack a glyph into the R8 atlas pages, allocating a new page on overflow.
    fn pack_into_pages(
        pages: &mut Vec<AtlasPage>,
        glyph: &RawGlyphImage,
        bpp: u32,
        label: &str,
    ) -> Option<GlyphEntry> {
        // Try the current (last) page first.
        let last_idx = pages.len() - 1;
        if let Some(mut entry) = pages[last_idx].try_pack(glyph) {
            entry.page = last_idx as u16;
            return Some(entry);
        }

        // Current page is full — allocate a new page if under the limit.
        if pages.len() >= MAX_ATLAS_PAGES {
            tracing::error!(
                "Atlas overflow: all {} pages full, cannot pack glyph for '{}'",
                MAX_ATLAS_PAGES, label
            );
            return None;
        }

        let new_page_idx = pages.len();
        tracing::info!(
            "Atlas page {} full, allocating page {} for glyph '{}'",
            last_idx, new_page_idx, label
        );
        pages.push(AtlasPage::new(bpp));

        if let Some(mut entry) = pages[new_page_idx].try_pack(glyph) {
            entry.page = new_page_idx as u16;
            Some(entry)
        } else {
            tracing::error!("Glyph '{}' too large for a fresh atlas page", label);
            None
        }
    }

    // --- Public rasterization methods ---

    /// Rasterize a single character and pack into the R8 atlas.
    fn rasterize(&mut self, c: char) -> Option<GlyphEntry> {
        let glyph = self.shape_and_extract_r8(&c.to_string())?;
        let entry = Self::pack_into_pages(
            &mut self.pages, &glyph, 1, &c.to_string(),
        )?;
        self.entries.insert(c, entry);
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

        let key_str: String = chars.iter().collect();

        // Check ligature cache first
        if let Some(entry) = self.ligature_entries.get(&key_str) {
            return Some(*entry);
        }

        // Shape and check if a ligature was applied (fewer glyphs than input chars)
        let buffer = self.shape_text(&key_str);
        let mut glyph_count = 0usize;
        for run in buffer.layout_runs() {
            glyph_count += run.glyphs.len();
            break; // first run only
        }
        if glyph_count == 0 || glyph_count >= chars.len() {
            return None;
        }

        tracing::trace!(ligature = %key_str, glyphs = glyph_count, "Ligature detected");

        let glyph = self.extract_first_glyph(&buffer, Self::convert_to_r8)?;
        let entry = Self::pack_into_pages(
            &mut self.pages, &glyph, 1, &key_str,
        )?;
        self.ligature_entries.insert(key_str, entry);
        Some(entry)
    }

    /// Get the subpixel RGBA atlas pixel data (page 0).
    pub fn subpixel_pixels(&self) -> &[u8] {
        &self.subpixel_pages[0].pixels
    }

    /// Number of subpixel RGBA atlas pages.
    pub fn subpixel_page_count(&self) -> usize {
        self.subpixel_pages.len()
    }

    /// Get the pixel data for a specific subpixel atlas page.
    pub fn subpixel_page_pixels(&self, page: usize) -> Option<&[u8]> {
        self.subpixel_pages.get(page).map(|p| p.pixels.as_slice())
    }

    /// Whether any subpixel atlas page needs re-upload.
    pub fn is_subpixel_dirty(&self) -> bool {
        self.subpixel_pages.iter().any(|p| p.dirty)
    }

    /// Mark all subpixel atlas pages as uploaded.
    pub fn clear_subpixel_dirty(&mut self) {
        for page in &mut self.subpixel_pages {
            page.dirty = false;
        }
    }

    /// Get indices of subpixel pages that need re-upload.
    pub fn dirty_subpixel_pages(&self) -> Vec<usize> {
        self.subpixel_pages.iter().enumerate()
            .filter(|(_, p)| p.dirty)
            .map(|(i, _)| i)
            .collect()
    }

    /// Look up or rasterize a glyph for subpixel rendering.
    /// Returns the glyph entry with UVs into the subpixel RGBA atlas.
    pub fn get_or_insert_subpixel(&mut self, c: char) -> Option<GlyphEntry> {
        if let Some(entry) = self.subpixel_entries.get(&c) {
            return Some(*entry);
        }
        if c.is_control() || c == ' ' {
            return None;
        }
        self.rasterize_subpixel(c)
    }

    /// Rasterize a character into the subpixel RGBA atlas.
    fn rasterize_subpixel(&mut self, c: char) -> Option<GlyphEntry> {
        let glyph = self.shape_and_extract_rgba(&c.to_string())?;
        let entry = Self::pack_into_pages(
            &mut self.subpixel_pages, &glyph, 4, &c.to_string(),
        )?;
        self.subpixel_entries.insert(c, entry);
        Some(entry)
    }

    /// Check if a character is a color emoji that should use the RGBA atlas.
    ///
    /// Covers Unicode emoji ranges including:
    /// - Emoticons (U+1F600..U+1F64F)
    /// - Misc symbols & pictographs (U+1F300..U+1F5FF)
    /// - Transport & map symbols (U+1F680..U+1F6FF)
    /// - Supplemental symbols (U+1F900..U+1F9FF, U+1FA00..U+1FA6F, U+1FA70..U+1FAFF)
    /// - Dingbats (U+2700..U+27BF)
    /// - Misc symbols (U+2600..U+26FF)
    /// - Variation selectors, ZWJ sequences handled by the caller
    pub fn is_color_emoji(c: char) -> bool {
        let cp = c as u32;
        matches!(cp,
            0x200D            |   // ZWJ
            0x231A..=0x231B   |   // Watch, hourglass
            0x23E9..=0x23F3   |   // Media controls
            0x23F8..=0x23FA   |   // More media controls
            0x25AA..=0x25AB   |   // Squares
            0x25B6 | 0x25C0   |   // Play/reverse
            0x25FB..=0x25FE   |   // Medium squares
            0x2600..=0x26FF   |   // Misc symbols (covers zodiac, weather, sports, etc.)
            0x2700..=0x27BF   |   // Dingbats (covers scissors, check marks, arrows, etc.)
            0xFE00..=0xFE0F   |   // Variation selectors
            0x1F300..=0x1F5FF |   // Misc symbols & pictographs
            0x1F600..=0x1F64F |   // Emoticons
            0x1F680..=0x1F6FF |   // Transport & map
            0x1F700..=0x1F77F |   // Alchemical
            0x1F780..=0x1F7FF |   // Geometric shapes ext
            0x1F800..=0x1F8FF |   // Supplemental arrows-C
            0x1F900..=0x1F9FF |   // Supplemental symbols
            0x1FA00..=0x1FA6F |   // Chess symbols
            0x1FA70..=0x1FAFF     // Symbols & pictographs ext-A
        )
    }

    /// Look up or rasterize a glyph, routing color emoji to the RGBA subpixel atlas.
    /// Returns `(entry, is_color)` where `is_color` indicates the RGBA atlas was used.
    pub fn get_or_insert_auto(&mut self, c: char) -> Option<(GlyphEntry, bool)> {
        if Self::is_color_emoji(c) {
            self.get_or_insert_subpixel(c).map(|e| (e, true))
        } else {
            self.get_or_insert(c).map(|e| (e, false))
        }
    }

    /// Pre-warm the atlas with ASCII printable characters.
    pub fn prewarm_ascii(&mut self) {
        for c in 0x20u8..=0x7Eu8 {
            self.get_or_insert(c as char);
        }
        tracing::debug!(
            glyphs = self.entries.len(),
            pages = self.pages.len(),
            "Atlas pre-warmed with ASCII glyphs"
        );
    }
}
