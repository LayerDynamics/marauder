/// iTerm2 inline image protocol parser.
///
/// The iTerm2 protocol transmits images via OSC 1337. The OSC payload has the
/// form:
///
/// ```text
/// File=[key=value;...]:BASE64DATA
/// ```
///
/// Keys: `name` (base64-encoded filename), `width`, `height`,
/// `preserveAspectRatio`, `inline`.
///
/// Dimension values: `auto`, `Npx` (pixels), `N%` (percent of terminal), or
/// a bare integer N (cells).
///
/// Reference: <https://iterm2.com/documentation-images.html>

/// How to interpret a dimension parameter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ImageDimension {
    /// Let the renderer decide (use natural image size).
    Auto,
    /// An explicit number of terminal cells.
    Cells(u32),
    /// An explicit pixel count.
    Pixels(u32),
    /// A percentage of the terminal width or height.
    Percent(u32),
}

/// Decoded parameters from an iTerm2 `File=...` OSC payload.
#[derive(Debug, Clone)]
pub struct ITermImageParams {
    /// Base64-decoded filename, if provided.
    pub name: Option<String>,
    pub width: ImageDimension,
    pub height: ImageDimension,
    pub preserve_aspect_ratio: bool,
    /// Whether the image should be displayed inline (`inline=1`).
    pub inline: bool,
}

/// Parse an iTerm2 OSC 1337 image payload.
///
/// `data` is everything after `"1337;"` in the OSC sequence — i.e. the full
/// `File=...` string including the base64 payload after `:`.
///
/// Returns `(params, raw_image_bytes)` on success, or `None` if the payload is
/// malformed.
pub fn parse_iterm2_image(data: &str) -> Option<(ITermImageParams, Vec<u8>)> {
    // Must start with "File=".
    let data = data.strip_prefix("File=")?;

    // Split at the first ':' to separate the key=value pairs from the payload.
    let (params_str, base64_data) = data.split_once(':')?;

    let mut params = ITermImageParams {
        name: None,
        width: ImageDimension::Auto,
        height: ImageDimension::Auto,
        preserve_aspect_ratio: true,
        inline: false,
    };

    for kv in params_str.split(';') {
        let kv = kv.trim();
        if kv.is_empty() {
            continue;
        }
        if let Some((key, value)) = kv.split_once('=') {
            match key.trim() {
                "name" => {
                    // The `name` value is itself base64-encoded.
                    if let Some(decoded) = decode_base64_to_string(value.trim()) {
                        params.name = Some(decoded);
                    }
                }
                "width" => params.width = parse_dimension(value.trim()),
                "height" => params.height = parse_dimension(value.trim()),
                "preserveAspectRatio" => {
                    params.preserve_aspect_ratio = value.trim() != "0";
                }
                "inline" => {
                    params.inline = value.trim() == "1";
                }
                // Unknown keys are silently ignored per the spec.
                _ => {}
            }
        }
    }

    // Decode the base64 image payload.
    let raw = decode_base64(base64_data.trim())?;
    Some((params, raw))
}

/// Parse a dimension string into `ImageDimension`.
///
/// Recognised formats:
/// - `"auto"` → `Auto`
/// - `"Npx"` → `Pixels(N)`
/// - `"N%"` → `Percent(N)`
/// - `"N"` (bare integer) → `Cells(N)`
pub fn parse_dimension(s: &str) -> ImageDimension {
    if s.eq_ignore_ascii_case("auto") {
        return ImageDimension::Auto;
    }
    if let Some(px) = s.strip_suffix("px") {
        if let Ok(n) = px.parse::<u32>() {
            return ImageDimension::Pixels(n);
        }
    }
    if let Some(pct) = s.strip_suffix('%') {
        if let Ok(n) = pct.parse::<u32>() {
            return ImageDimension::Percent(n);
        }
    }
    if let Ok(n) = s.parse::<u32>() {
        return ImageDimension::Cells(n);
    }
    // Unrecognised → treat as auto.
    ImageDimension::Auto
}

// ---------------------------------------------------------------------------
// Minimal base64 decoder (no external dependency)
// ---------------------------------------------------------------------------

/// Standard base64 alphabet → 6-bit value, or 0xFF for `=` padding / invalid.
fn base64_value(b: u8) -> u8 {
    match b {
        b'A'..=b'Z' => b - b'A',
        b'a'..=b'z' => b - b'a' + 26,
        b'0'..=b'9' => b - b'0' + 52,
        b'+' => 62,
        b'/' => 63,
        // URL-safe alternatives are also accepted.
        b'-' => 62,
        b'_' => 63,
        // Padding and whitespace — skip / signal end.
        _ => 0xFF,
    }
}

/// Return the 6-bit value for a base64 character, or `None` for invalid input.
/// This is the checked wrapper around `base64_value` that rejects 0xFF.
#[inline]
fn base64_checked(b: u8) -> Option<u8> {
    let v = base64_value(b);
    if v == 0xFF {
        None
    } else {
        Some(v)
    }
}

/// Decode a base64 string to raw bytes. Returns `None` on structural errors
/// or invalid characters. Whitespace and `=` padding are stripped first.
pub fn decode_base64(s: &str) -> Option<Vec<u8>> {
    // Collect non-whitespace, non-padding bytes.
    let chars: Vec<u8> = s
        .bytes()
        .filter(|&b| !b.is_ascii_whitespace() && b != b'=')
        .collect();

    // Length of clean input must be 0, 2, 3, or 4 (mod 4 residue).
    let rem = chars.len() % 4;
    if rem == 1 {
        // 1 leftover base64 char encodes only 6 bits — not enough for a byte.
        return None;
    }

    let full_groups = chars.len() / 4;
    let extra = match rem {
        0 => 0,
        2 => 1, // 2 chars → 1 byte
        3 => 2, // 3 chars → 2 bytes
        _ => unreachable!(),
    };
    let mut out = Vec::with_capacity(full_groups * 3 + extra);

    let mut idx = 0;
    // Full 4-char groups.
    for _ in 0..full_groups {
        let b0 = base64_checked(chars[idx])?;
        let b1 = base64_checked(chars[idx + 1])?;
        let b2 = base64_checked(chars[idx + 2])?;
        let b3 = base64_checked(chars[idx + 3])?;
        idx += 4;
        out.push((b0 << 2) | (b1 >> 4));
        out.push((b1 << 4) | (b2 >> 2));
        out.push((b2 << 6) | b3);
    }
    // Remaining partial group.
    if rem == 2 {
        let b0 = base64_checked(chars[idx])?;
        let b1 = base64_checked(chars[idx + 1])?;
        out.push((b0 << 2) | (b1 >> 4));
    } else if rem == 3 {
        let b0 = base64_checked(chars[idx])?;
        let b1 = base64_checked(chars[idx + 1])?;
        let b2 = base64_checked(chars[idx + 2])?;
        out.push((b0 << 2) | (b1 >> 4));
        out.push((b1 << 4) | (b2 >> 2));
    }

    Some(out)
}

/// Convenience: decode base64 and interpret the bytes as UTF-8. Returns `None`
/// if decoding fails or the result is not valid UTF-8.
fn decode_base64_to_string(s: &str) -> Option<String> {
    let bytes = decode_base64(s)?;
    String::from_utf8(bytes).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    // --- parse_dimension ---

    #[test]
    fn test_dimension_auto() {
        assert_eq!(parse_dimension("auto"), ImageDimension::Auto);
        assert_eq!(parse_dimension("Auto"), ImageDimension::Auto);
        assert_eq!(parse_dimension("AUTO"), ImageDimension::Auto);
    }

    #[test]
    fn test_dimension_pixels() {
        assert_eq!(parse_dimension("200px"), ImageDimension::Pixels(200));
        assert_eq!(parse_dimension("0px"), ImageDimension::Pixels(0));
    }

    #[test]
    fn test_dimension_percent() {
        assert_eq!(parse_dimension("50%"), ImageDimension::Percent(50));
        assert_eq!(parse_dimension("100%"), ImageDimension::Percent(100));
    }

    #[test]
    fn test_dimension_cells() {
        assert_eq!(parse_dimension("80"), ImageDimension::Cells(80));
        assert_eq!(parse_dimension("1"), ImageDimension::Cells(1));
    }

    #[test]
    fn test_dimension_unknown() {
        assert_eq!(parse_dimension(""), ImageDimension::Auto);
        assert_eq!(parse_dimension("garbage"), ImageDimension::Auto);
    }

    // --- decode_base64 ---

    #[test]
    fn test_base64_hello() {
        // "hello" = aGVsbG8=
        let decoded = decode_base64("aGVsbG8=").expect("should decode");
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_base64_padding_two() {
        // "Ma" (2 bytes) = TWE=
        let decoded = decode_base64("TWE=").expect("should decode");
        assert_eq!(decoded, b"Ma");
    }

    #[test]
    fn test_base64_no_padding() {
        // Without padding characters — should still work.
        let decoded = decode_base64("aGVsbG8").expect("should decode");
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_base64_empty() {
        let decoded = decode_base64("").expect("should decode empty");
        assert!(decoded.is_empty());
    }

    #[test]
    fn test_base64_whitespace_ignored() {
        // Newlines and spaces inside the base64 string should be ignored.
        let decoded = decode_base64("aGVs\nbG8=").expect("should decode");
        assert_eq!(decoded, b"hello");
    }

    #[test]
    fn test_base64_url_safe() {
        // URL-safe base64 uses `-` and `_` instead of `+` and `/`.
        // "f\xfb\xff" → base64 standard "+/v/" → url-safe "-_v_"
        let decoded_std = decode_base64("+/v/").expect("std");
        let decoded_url = decode_base64("-_v_").expect("url-safe");
        assert_eq!(decoded_std, decoded_url);
    }

    #[test]
    fn test_base64_invalid_length() {
        // A single base64 char is structurally invalid (1 mod 4 == 1).
        assert!(decode_base64("a").is_none());
    }

    #[test]
    fn test_base64_invalid_characters() {
        // Invalid chars like '!' should cause None, not silent corruption.
        assert!(decode_base64("aG!l").is_none());
        assert!(decode_base64("@@@@").is_none());
        assert!(decode_base64("aGVsbG8#").is_none());
    }

    #[test]
    fn test_parse_iterm2_invalid_base64_payload() {
        // Invalid base64 in the image payload should return None.
        let payload = "File=inline=1:!!!INVALID!!!";
        assert!(parse_iterm2_image(payload).is_none());
    }

    // --- parse_iterm2_image ---

    fn encode_base64(data: &[u8]) -> String {
        const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
        let mut out = String::new();
        let mut i = 0;
        while i + 3 <= data.len() {
            let n = ((data[i] as u32) << 16)
                | ((data[i + 1] as u32) << 8)
                | (data[i + 2] as u32);
            out.push(ALPHABET[(n >> 18) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
            out.push(ALPHABET[(n & 0x3F) as usize] as char);
            i += 3;
        }
        let rem = data.len() - i;
        if rem == 1 {
            let n = (data[i] as u32) << 16;
            out.push(ALPHABET[(n >> 18) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            out.push_str("==");
        } else if rem == 2 {
            let n = ((data[i] as u32) << 16) | ((data[i + 1] as u32) << 8);
            out.push(ALPHABET[(n >> 18) as usize] as char);
            out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
            out.push(ALPHABET[((n >> 6) & 0x3F) as usize] as char);
            out.push('=');
        }
        out
    }

    #[test]
    fn test_parse_basic_iterm2() {
        let image_data = b"PNG_FAKE_DATA";
        let b64 = encode_base64(image_data);
        let payload = format!("File=inline=1;width=80;height=24:{}", b64);
        let (params, raw) = parse_iterm2_image(&payload).expect("should parse");
        assert!(params.inline);
        assert_eq!(params.width, ImageDimension::Cells(80));
        assert_eq!(params.height, ImageDimension::Cells(24));
        assert!(params.preserve_aspect_ratio); // default
        assert_eq!(&raw, image_data);
    }

    #[test]
    fn test_parse_iterm2_with_name() {
        let image_data = b"\x89PNG";
        let b64_image = encode_base64(image_data);
        // name is base64-encoded "test.png"
        let b64_name = encode_base64(b"test.png");
        let payload = format!("File=name={};inline=1:{}", b64_name, b64_image);
        let (params, raw) = parse_iterm2_image(&payload).expect("should parse");
        assert_eq!(params.name.as_deref(), Some("test.png"));
        assert_eq!(&raw, image_data);
    }

    #[test]
    fn test_parse_iterm2_pixel_dimensions() {
        let b64 = encode_base64(b"data");
        let payload = format!("File=width=200px;height=100px:{}", b64);
        let (params, _) = parse_iterm2_image(&payload).expect("should parse");
        assert_eq!(params.width, ImageDimension::Pixels(200));
        assert_eq!(params.height, ImageDimension::Pixels(100));
    }

    #[test]
    fn test_parse_iterm2_percent_dimensions() {
        let b64 = encode_base64(b"data");
        let payload = format!("File=width=50%;height=25%:{}", b64);
        let (params, _) = parse_iterm2_image(&payload).expect("should parse");
        assert_eq!(params.width, ImageDimension::Percent(50));
        assert_eq!(params.height, ImageDimension::Percent(25));
    }

    #[test]
    fn test_parse_iterm2_no_aspect_ratio() {
        let b64 = encode_base64(b"d");
        let payload = format!("File=preserveAspectRatio=0:{}", b64);
        let (params, _) = parse_iterm2_image(&payload).expect("should parse");
        assert!(!params.preserve_aspect_ratio);
    }

    #[test]
    fn test_parse_iterm2_auto_dimensions() {
        let b64 = encode_base64(b"d");
        let payload = format!("File=width=auto;height=auto:{}", b64);
        let (params, _) = parse_iterm2_image(&payload).expect("should parse");
        assert_eq!(params.width, ImageDimension::Auto);
        assert_eq!(params.height, ImageDimension::Auto);
    }

    #[test]
    fn test_parse_iterm2_missing_file_prefix() {
        // Must start with "File="
        assert!(parse_iterm2_image("inline=1:data").is_none());
    }

    #[test]
    fn test_parse_iterm2_missing_colon() {
        // No ':' separator → None
        assert!(parse_iterm2_image("File=inline=1").is_none());
    }

    #[test]
    fn test_parse_iterm2_empty_params() {
        let b64 = encode_base64(b"bytes");
        let payload = format!("File=:{}", b64);
        let (params, raw) = parse_iterm2_image(&payload).expect("should parse with empty params");
        assert!(!params.inline);
        assert_eq!(params.width, ImageDimension::Auto);
        assert_eq!(&raw, b"bytes");
    }

    #[test]
    fn test_parse_iterm2_unknown_keys_ignored() {
        let b64 = encode_base64(b"x");
        let payload = format!("File=unknownKey=someValue;inline=1:{}", b64);
        let (params, _) = parse_iterm2_image(&payload).expect("should parse");
        assert!(params.inline);
    }
}
