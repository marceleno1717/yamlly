//! Lightweight, standard RFC 4648 Base64 encoding and decoding utilities.

const B64_ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encode a byte slice into a standard Base64 string with padding.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        let n = ((b0 as u32) << 16) | ((b1 as u32) << 8) | (b2 as u32);
        out.push(B64_ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(B64_ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_ALPHABET[((n >> 6) & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_ALPHABET[(n & 0x3F) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Decode a Base64 string into raw bytes, ignoring whitespace.
pub fn base64_decode(input: &str) -> Result<Vec<u8>, &'static str> {
    let clean: Vec<u8> = input.bytes().filter(|b| !b.is_ascii_whitespace()).collect();
    if clean.is_empty() {
        return Ok(Vec::new());
    }
    if !clean.len().is_multiple_of(4) {
        return Err("invalid base64 length");
    }

    let decode_char = |b: u8| -> Result<u8, &'static str> {
        match b {
            b'A'..=b'Z' => Ok(b - b'A'),
            b'a'..=b'z' => Ok(b - b'a' + 26),
            b'0'..=b'9' => Ok(b - b'0' + 52),
            b'+' | b'-' => Ok(62),
            b'/' | b'_' => Ok(63),
            b'=' => Ok(0),
            _ => Err("invalid base64 character"),
        }
    };

    let mut out = Vec::with_capacity(clean.len() / 4 * 3);
    for chunk in clean.as_chunks::<4>().0 {
        let d0 = decode_char(chunk[0])?;
        let d1 = decode_char(chunk[1])?;
        let d2 = decode_char(chunk[2])?;
        let d3 = decode_char(chunk[3])?;

        let n = ((d0 as u32) << 18) | ((d1 as u32) << 12) | ((d2 as u32) << 6) | (d3 as u32);
        out.push(((n >> 16) & 0xFF) as u8);
        if chunk[2] != b'=' {
            out.push(((n >> 8) & 0xFF) as u8);
        }
        if chunk[3] != b'=' {
            out.push((n & 0xFF) as u8);
        }
    }
    Ok(out)
}

/// Decode a Base64 string directly into a UTF-8 string.
pub fn base64_decode_utf8(input: &str) -> Result<String, &'static str> {
    let bytes = base64_decode(input)?;
    String::from_utf8(bytes).map_err(|_| "decoded bytes are not valid UTF-8")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rfc_4648_test_vectors() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"foob"), "Zm9vYg==");
        assert_eq!(base64_encode(b"fooba"), "Zm9vYmE=");
        assert_eq!(base64_encode(b"foobar"), "Zm9vYmFy");

        assert_eq!(base64_decode_utf8("").unwrap(), "");
        assert_eq!(base64_decode_utf8("Zg==").unwrap(), "f");
        assert_eq!(base64_decode_utf8("Zm8=").unwrap(), "fo");
        assert_eq!(base64_decode_utf8("Zm9v").unwrap(), "foo");
        assert_eq!(base64_decode_utf8("Zm9vYg==").unwrap(), "foob");
        assert_eq!(base64_decode_utf8("Zm9vYmE=").unwrap(), "fooba");
        assert_eq!(base64_decode_utf8("Zm9vYmFy").unwrap(), "foobar");
    }

    #[test]
    fn test_unicode_and_whitespace() {
        let original = "Hello, 世界! 🔑 Secret 123";
        let encoded = base64_encode(original.as_bytes());
        let decoded = base64_decode_utf8(&encoded).unwrap();
        assert_eq!(decoded, original);

        // Whitespace tolerance in decoding
        let with_spaces = format!("  \n  {encoded}  \t\n");
        assert_eq!(base64_decode_utf8(&with_spaces).unwrap(), original);
    }
}
