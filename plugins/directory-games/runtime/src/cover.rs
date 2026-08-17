use asset_plugin_sdk::{Error, Result, Value, decode_base64};
use image::{ImageFormat, ImageReader, Limits};
use std::io::Cursor;

const MAX_COVER_SIZE: usize = 1024 * 1024;
const MAX_COVER_DIMENSION: u32 = 4096;
const MAX_DECODED_COVER_SIZE: u64 = 64 * 1024 * 1024;

/// 已校验并确定存储信息的游戏封面。
pub(crate) struct GameIcon {
    pub(crate) filename: &'static str,
    pub(crate) mime_type: &'static str,
    pub(crate) bytes: Vec<u8>,
}

/// 从 Action 输入中读取可选封面，并按实际内容完成校验和规范化。
pub(crate) fn optional_icon(input: &Value) -> Result<Option<GameIcon>> {
    let Some(icon) = input.get("icon") else {
        return Ok(None);
    };
    if icon.is_null() {
        return Ok(None);
    }
    let icon = icon
        .as_object()
        .ok_or_else(|| Error::msg("icon must be an object"))?;
    let data = icon
        .get("data")
        .and_then(Value::as_str)
        .ok_or_else(|| Error::msg("icon.data is required"))?;
    if data.len() > MAX_COVER_SIZE.saturating_mul(4) / 3 + 4 {
        return Err(Error::msg("game icon exceeds 1 MiB").into());
    }
    let bytes = decode_base64(data).map_err(|_| Error::msg("game icon is not valid base64"))?;
    if bytes.is_empty() {
        return Err(Error::msg("game icon must not be empty").into());
    }
    if bytes.len() > MAX_COVER_SIZE {
        return Err(Error::msg("game icon exceeds 1 MiB").into());
    }

    detect_icon(bytes).map(Some)
}

/// 根据文件内容识别受支持的图片格式并生成规范存储信息。
fn detect_icon(bytes: Vec<u8>) -> Result<GameIcon> {
    let format = match image::guess_format(&bytes) {
        Ok(format) => format,
        Err(_) => {
            let bytes = normalize_svg(&bytes)?;
            return Ok(GameIcon {
                filename: "cover.svg",
                mime_type: "image/svg+xml",
                bytes,
            });
        }
    };
    let (filename, mime_type) = match format {
        ImageFormat::Png => ("cover.png", "image/png"),
        ImageFormat::Jpeg => ("cover.jpg", "image/jpeg"),
        ImageFormat::WebP => ("cover.webp", "image/webp"),
        ImageFormat::Gif => ("cover.gif", "image/gif"),
        _ => return Err(Error::msg("game icon must be PNG, JPEG, WebP, GIF, or SVG").into()),
    };
    validate_raster_image(&bytes, format)?;
    Ok(GameIcon {
        filename,
        mime_type,
        bytes,
    })
}

/// 完整解码位图，并限制图片尺寸和解码内存。
fn validate_raster_image(bytes: &[u8], format: ImageFormat) -> Result<()> {
    let mut limits = Limits::default();
    limits.max_image_width = Some(MAX_COVER_DIMENSION);
    limits.max_image_height = Some(MAX_COVER_DIMENSION);
    limits.max_alloc = Some(MAX_DECODED_COVER_SIZE);
    let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
    reader.limits(limits);
    reader.decode().map_err(|error| {
        Error::msg(format!(
            "game icon is not a valid {} image: {error}",
            format.to_mime_type()
        ))
    })?;
    Ok(())
}

/// 将 SVG 解析为静态树，检查尺寸后输出规范化内容。
fn normalize_svg(bytes: &[u8]) -> Result<Vec<u8>> {
    let options = usvg::Options::default();
    let tree = usvg::Tree::from_data_nested(bytes, &options)
        .map_err(|error| Error::msg(format!("game icon is not a valid SVG image: {error}")))?;
    let size = tree.size();
    if size.width() > MAX_COVER_DIMENSION as f32 || size.height() > MAX_COVER_DIMENSION as f32 {
        return Err(Error::msg(format!(
            "game icon dimensions exceed {MAX_COVER_DIMENSION}x{MAX_COVER_DIMENSION}"
        ))
        .into());
    }
    let normalized = tree.to_string(&usvg::WriteOptions::default()).into_bytes();
    if normalized.len() > MAX_COVER_SIZE {
        return Err(Error::msg("normalized game icon exceeds 1 MiB").into());
    }
    Ok(normalized)
}

#[cfg(test)]
mod tests {
    use super::optional_icon;
    use asset_plugin_sdk::{json, runtime::encode_base64};
    use image::{DynamicImage, ImageFormat};
    use std::io::Cursor;

    /// 验证位图以真实内容确定格式，并拒绝无法完整解码的数据。
    #[test]
    fn raster_content_controls_its_identity_and_must_decode() {
        let mut png = Cursor::new(Vec::new());
        DynamicImage::new_rgba8(1, 1)
            .write_to(&mut png, ImageFormat::Png)
            .unwrap();
        let icon = optional_icon(&json!({
            "icon": {
                "mime_type": "image/jpeg",
                "data": encode_base64(png.into_inner())
            }
        }))
        .unwrap()
        .unwrap();
        assert_eq!(icon.filename, "cover.png");
        assert_eq!(icon.mime_type, "image/png");

        let invalid = optional_icon(&json!({
            "icon": {
                "mime_type": "image/png",
                "data": encode_base64(b"\x89PNG\r\n\x1a\n")
            }
        }));
        assert!(invalid.is_err());
    }

    /// 验证 SVG 会规范化，并移除动态内容和外部文件引用。
    #[test]
    fn svg_content_is_normalized_without_dynamic_or_external_content() {
        let source = br#"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32">
            <script>alert('no')</script>
            <image href="secret.png" width="32" height="32"/>
            <rect width="32" height="32" fill="red"/>
        </svg>"#;
        let icon = optional_icon(&json!({
            "icon": {
                "mime_type": "image/png",
                "data": encode_base64(source)
            }
        }))
        .unwrap()
        .unwrap();
        let normalized = String::from_utf8(icon.bytes).unwrap();

        assert_eq!(icon.filename, "cover.svg");
        assert_eq!(icon.mime_type, "image/svg+xml");
        assert!(normalized.contains("<svg"));
        assert!(normalized.contains("<path"));
        assert!(!normalized.contains("script"));
        assert!(!normalized.contains("secret.png"));
    }
}
