//! Bounded header parsing for the intrinsic `core:image` width and height fields.
//!
//! This intentionally reads only format headers and never decodes pixels. Unsupported or malformed
//! images simply produce no derived layer, so metadata extraction cannot make an otherwise valid
//! resource unreadable.

pub(super) fn dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    png_dimensions(bytes)
        .or_else(|| gif_dimensions(bytes))
        .or_else(|| jpeg_dimensions(bytes))
        .or_else(|| webp_dimensions(bytes))
        .or_else(|| bmp_dimensions(bytes))
        .or_else(|| avif_dimensions(bytes))
        .filter(|(width, height)| *width > 0 && *height > 0)
}

fn png_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..8)? != b"\x89PNG\r\n\x1a\n" || bytes.get(12..16)? != b"IHDR" {
        return None;
    }
    Some((be_u32(bytes, 16)?, be_u32(bytes, 20)?))
}

fn gif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !matches!(bytes.get(..6)?, b"GIF87a" | b"GIF89a") {
        return None;
    }
    Some((u32::from(le_u16(bytes, 6)?), u32::from(le_u16(bytes, 8)?)))
}

fn jpeg_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..2)? != b"\xff\xd8" {
        return None;
    }
    let mut cursor = 2usize;
    while cursor < bytes.len() {
        while bytes.get(cursor) == Some(&0xff) {
            cursor += 1;
        }
        let marker = *bytes.get(cursor)?;
        cursor += 1;
        if marker == 0x00 || marker == 0xd8 || marker == 0xd9 || (0xd0..=0xd7).contains(&marker) {
            continue;
        }
        let length = usize::from(be_u16(bytes, cursor)?);
        if length < 2 || cursor.checked_add(length)? > bytes.len() {
            return None;
        }
        if matches!(
            marker,
            0xc0 | 0xc1
                | 0xc2
                | 0xc3
                | 0xc5
                | 0xc6
                | 0xc7
                | 0xc9
                | 0xca
                | 0xcb
                | 0xcd
                | 0xce
                | 0xcf
        ) {
            if length < 7 {
                return None;
            }
            let height = u32::from(be_u16(bytes, cursor + 3)?);
            let width = u32::from(be_u16(bytes, cursor + 5)?);
            return Some((width, height));
        }
        cursor += length;
    }
    None
}

fn webp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..4)? != b"RIFF" || bytes.get(8..12)? != b"WEBP" {
        return None;
    }
    let chunk = bytes.get(12..16)?;
    let data = 20usize;
    match chunk {
        b"VP8X" => Some((1 + le_u24(bytes, data + 4)?, 1 + le_u24(bytes, data + 7)?)),
        b"VP8 " => {
            if bytes.get(data + 3..data + 6)? != b"\x9d\x01\x2a" {
                return None;
            }
            Some((
                u32::from(le_u16(bytes, data + 6)? & 0x3fff),
                u32::from(le_u16(bytes, data + 8)? & 0x3fff),
            ))
        }
        b"VP8L" => {
            if bytes.get(data) != Some(&0x2f) {
                return None;
            }
            let b1 = u32::from(*bytes.get(data + 1)?);
            let b2 = u32::from(*bytes.get(data + 2)?);
            let b3 = u32::from(*bytes.get(data + 3)?);
            let b4 = u32::from(*bytes.get(data + 4)?);
            Some((
                1 + b1 + ((b2 & 0x3f) << 8),
                1 + (b2 >> 6) + (b3 << 2) + ((b4 & 0x0f) << 10),
            ))
        }
        _ => None,
    }
}

fn bmp_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if bytes.get(..2)? != b"BM" {
        return None;
    }
    match le_u32(bytes, 14)? {
        12 => Some((u32::from(le_u16(bytes, 18)?), u32::from(le_u16(bytes, 20)?))),
        size if size >= 40 => {
            let width = le_i32(bytes, 18)?.unsigned_abs();
            let height = le_i32(bytes, 22)?.unsigned_abs();
            Some((width, height))
        }
        _ => None,
    }
}

fn avif_dimensions(bytes: &[u8]) -> Option<(u32, u32)> {
    if !bytes.windows(4).any(|window| window == b"ftyp")
        || !bytes
            .windows(4)
            .any(|window| matches!(window, b"avif" | b"avis" | b"mif1"))
    {
        return None;
    }
    for (offset, window) in bytes.windows(4).enumerate() {
        if window != b"ispe" || offset < 4 {
            continue;
        }
        let size = usize::try_from(be_u32(bytes, offset - 4)?).ok()?;
        if size < 20 || offset.checked_add(16)? > bytes.len() {
            continue;
        }
        let width = be_u32(bytes, offset + 8)?;
        let height = be_u32(bytes, offset + 12)?;
        if width > 0 && height > 0 {
            return Some((width, height));
        }
    }
    None
}

fn be_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_be_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn le_u16(bytes: &[u8], offset: usize) -> Option<u16> {
    Some(u16::from_le_bytes(
        bytes.get(offset..offset + 2)?.try_into().ok()?,
    ))
}

fn be_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_be_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_u32(bytes: &[u8], offset: usize) -> Option<u32> {
    Some(u32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_i32(bytes: &[u8], offset: usize) -> Option<i32> {
    Some(i32::from_le_bytes(
        bytes.get(offset..offset + 4)?.try_into().ok()?,
    ))
}

fn le_u24(bytes: &[u8], offset: usize) -> Option<u32> {
    let value = bytes.get(offset..offset + 3)?;
    Some(u32::from(value[0]) | (u32::from(value[1]) << 8) | (u32::from(value[2]) << 16))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_common_raster_dimensions_without_decoding_pixels() {
        let mut png = vec![0; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        png[16..20].copy_from_slice(&1920_u32.to_be_bytes());
        png[20..24].copy_from_slice(&1080_u32.to_be_bytes());
        assert_eq!(dimensions(&png), Some((1920, 1080)));

        let mut gif = b"GIF89a".to_vec();
        gif.extend_from_slice(&640_u16.to_le_bytes());
        gif.extend_from_slice(&480_u16.to_le_bytes());
        assert_eq!(dimensions(&gif), Some((640, 480)));

        let mut bmp = vec![0; 26];
        bmp[..2].copy_from_slice(b"BM");
        bmp[14..18].copy_from_slice(&40_u32.to_le_bytes());
        bmp[18..22].copy_from_slice(&800_i32.to_le_bytes());
        bmp[22..26].copy_from_slice(&(-600_i32).to_le_bytes());
        assert_eq!(dimensions(&bmp), Some((800, 600)));

        let mut jpeg = vec![0; 21];
        jpeg[..4].copy_from_slice(b"\xff\xd8\xff\xc0");
        jpeg[4..6].copy_from_slice(&17_u16.to_be_bytes());
        jpeg[6] = 8;
        jpeg[7..9].copy_from_slice(&720_u16.to_be_bytes());
        jpeg[9..11].copy_from_slice(&1280_u16.to_be_bytes());
        assert_eq!(dimensions(&jpeg), Some((1280, 720)));

        let mut webp = vec![0; 30];
        webp[..4].copy_from_slice(b"RIFF");
        webp[8..12].copy_from_slice(b"WEBP");
        webp[12..16].copy_from_slice(b"VP8X");
        webp[24..27].copy_from_slice(&[0xff, 0x03, 0x00]);
        webp[27..30].copy_from_slice(&[0x3f, 0x02, 0x00]);
        assert_eq!(dimensions(&webp), Some((1024, 576)));

        let mut avif = vec![0; 36];
        avif[4..8].copy_from_slice(b"ftyp");
        avif[8..12].copy_from_slice(b"avif");
        avif[16..20].copy_from_slice(&20_u32.to_be_bytes());
        avif[20..24].copy_from_slice(b"ispe");
        avif[28..32].copy_from_slice(&3840_u32.to_be_bytes());
        avif[32..36].copy_from_slice(&2160_u32.to_be_bytes());
        assert_eq!(dimensions(&avif), Some((3840, 2160)));
    }

    #[test]
    fn rejects_truncated_or_zero_sized_headers() {
        assert_eq!(dimensions(b"\x89PNG"), None);
        let mut png = vec![0; 24];
        png[..8].copy_from_slice(b"\x89PNG\r\n\x1a\n");
        png[12..16].copy_from_slice(b"IHDR");
        assert_eq!(dimensions(&png), None);
    }
}
