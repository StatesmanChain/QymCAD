//! Bounded image inputs, re-encoded without file metadata before transmission.
use super::*;
use base64::{Engine, engine::general_purpose::STANDARD};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader};
use std::{
    io::{Cursor, Read},
    path::Path,
    sync::Arc,
};

pub const MAX_IMAGES: usize = 4;
const MAX_SOURCE_BYTES: usize = 20 * 1024 * 1024;
const MAX_IMAGE_BYTES: usize = 10 * 1024 * 1024;
const MAX_TOTAL_BYTES: usize = 20 * 1024 * 1024;
const MAX_PIXELS: u64 = 24_000_000;

#[derive(Clone)]
pub struct ImageInput {
    data: Arc<ImageData>,
}
struct ImageData {
    name: String,
    url: String,
    bytes: usize,
    thumbnail: image::RgbaImage,
}
impl ImageInput {
    pub fn read(path: &Path) -> Result<Self, &'static str> {
        let meta = std::fs::metadata(path).map_err(|_| "ai-error-image-read")?;
        if !meta.is_file() || meta.len() > MAX_SOURCE_BYTES as u64 {
            return Err("ai-error-image-size");
        }
        let mut bytes = Vec::new();
        std::fs::File::open(path).map_err(|_| "ai-error-image-read")?.take(MAX_SOURCE_BYTES as u64 + 1).read_to_end(&mut bytes).map_err(|_| "ai-error-image-read")?;
        let name = path.file_name().ok_or("ai-error-image-read")?.to_string_lossy();
        Self::from_bytes(&bytes, &name)
    }
    pub fn from_bytes(bytes: &[u8], name: &str) -> Result<Self, &'static str> {
        if bytes.len() > MAX_SOURCE_BYTES {
            return Err("ai-error-image-size");
        }
        let format = image::guess_format(bytes).map_err(|_| "ai-error-image-format")?;
        if !matches!(format, ImageFormat::Png | ImageFormat::Jpeg | ImageFormat::WebP) {
            return Err("ai-error-image-format");
        }
        let mut limits = image::Limits::default();
        limits.max_image_width = Some(8192);
        limits.max_image_height = Some(8192);
        limits.max_alloc = Some(160 * 1024 * 1024);
        let mut reader = ImageReader::with_format(Cursor::new(bytes), format);
        reader.limits(limits);
        let mut decoder = reader.into_decoder().map_err(|_| "ai-error-image-format")?;
        let size = decoder.dimensions();
        if u64::from(size.0) * u64::from(size.1) > MAX_PIXELS || size.0 == 0 || size.1 == 0 {
            return Err("ai-error-image-size");
        }
        let orientation = decoder.orientation().map_err(|_| "ai-error-image-format")?;
        let mut decoded = DynamicImage::from_decoder(decoder).map_err(|_| "ai-error-image-format")?;
        decoded.apply_orientation(orientation);
        let mut output = Cursor::new(Vec::new());
        let mime = if format == ImageFormat::Jpeg {
            image::codecs::jpeg::JpegEncoder::new_with_quality(&mut output, 95).encode_image(&decoded.to_rgb8()).map_err(|_| "ai-error-image-format")?;
            "image/jpeg"
        } else {
            decoded.write_to(&mut output, ImageFormat::Png).map_err(|_| "ai-error-image-format")?;
            "image/png"
        };
        let output = output.into_inner();
        if output.len() > MAX_IMAGE_BYTES {
            return Err("ai-error-image-size");
        }
        let name: String = name.chars().filter(|c| !c.is_control()).take(160).collect();
        Ok(Self {
            data: Arc::new(ImageData {
                name,
                url: format!("data:{mime};base64,{}", STANDARD.encode(&output)),
                bytes: output.len(),
                thumbnail: decoded.thumbnail(decoded.width().min(512), decoded.height().min(512)).to_rgba8(),
            }),
        })
    }
    pub fn name(&self) -> &str {
        &self.data.name
    }
    pub fn thumbnail(&self) -> &image::RgbaImage {
        &self.data.thumbnail
    }
}
pub(crate) fn validate_set(images: &[ImageInput]) -> Result<(), &'static str> {
    if images.len() > MAX_IMAGES || images.iter().map(|i| i.data.bytes).sum::<usize>() > MAX_TOTAL_BYTES {
        Err("ai-error-image-total")
    } else {
        Ok(())
    }
}
pub(crate) fn content(input: &Request<'_>) -> serde_json::Value {
    if input.images.is_empty() {
        return serde_json::json!(input.prompt);
    }
    let mut parts = vec![serde_json::json!({"type":"text","text":input.prompt})];
    for image in input.images {
        parts.push(serde_json::json!({"type":"image_url","image_url":{"url":image.data.url,"detail":"high"}}));
    }
    serde_json::json!(parts)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn png() -> Vec<u8> {
        let mut out = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(120, 80).write_to(&mut out, ImageFormat::Png).unwrap();
        out.into_inner()
    }
    #[test]
    fn images_are_sent_as_high_detail_data_not_file_paths() {
        let reference = ImageInput::from_bytes(&png(), "drawing.png").unwrap();
        let images = [reference];
        let content = content(&Request { prompt: "Build the dimensioned plate", images: &images });
        assert_eq!(content[0]["type"], "text");
        assert_eq!(content[1]["image_url"]["detail"], "high");
        let url = content[1]["image_url"]["url"].as_str().unwrap();
        let encoded = url.strip_prefix("data:image/png;base64,").unwrap();
        assert_eq!(image::load_from_memory(&STANDARD.decode(encoded).unwrap()).unwrap().width(), 120);
        assert!(!content.to_string().contains("drawing.png"));
    }
    #[test]
    fn corrupt_unsupported_and_oversized_images_are_rejected() {
        assert!(ImageInput::from_bytes(b"%PDF-1.7", "drawing.pdf").is_err());
        assert!(ImageInput::from_bytes(&png()[..20], "broken.png").is_err());
        assert!(ImageInput::from_bytes(&vec![0; MAX_SOURCE_BYTES + 1], "huge.png").is_err());
        let item = ImageInput::from_bytes(&png(), "one.png").unwrap();
        assert!(validate_set(&vec![item; MAX_IMAGES + 1]).is_err());
    }
    #[test]
    fn jpeg_and_webp_are_decoded_and_text_requests_stay_text() {
        for format in [ImageFormat::Jpeg, ImageFormat::WebP] {
            let mut bytes = Cursor::new(Vec::new());
            DynamicImage::new_rgb8(20, 30).write_to(&mut bytes, format).unwrap();
            let item = ImageInput::from_bytes(bytes.get_ref(), "photo").unwrap();
            assert_eq!(item.thumbnail().height(), 30);
        }
        assert_eq!(content(&Request { prompt: "plate", images: &[] }), "plate");
    }
    #[test]
    fn phone_orientation_is_applied_and_exif_is_removed() {
        let mut jpeg = Cursor::new(Vec::new());
        DynamicImage::new_rgb8(20, 30).write_to(&mut jpeg, ImageFormat::Jpeg).unwrap();
        let mut bytes = jpeg.into_inner();
        // APP1: little-endian TIFF with one SHORT orientation tag, rotate 90 degrees.
        let exif = [0xff, 0xe1, 0, 34, b'E', b'x', b'i', b'f', 0, 0, b'I', b'I', 42, 0, 8, 0, 0, 0, 1, 0, 0x12, 1, 3, 0, 1, 0, 0, 0, 6, 0, 0, 0, 0, 0, 0, 0];
        bytes.splice(2..2, exif);
        let input = ImageInput::from_bytes(&bytes, "phone.jpg").unwrap();
        assert_eq!(input.thumbnail().width(), 30);
        assert_eq!(input.thumbnail().height(), 20);
        let encoded = input.data.url.strip_prefix("data:image/jpeg;base64,").unwrap();
        let normalized = STANDARD.decode(encoded).unwrap();
        assert!(!normalized.windows(6).any(|w| w == b"Exif\0\0"));
        assert_eq!(image::load_from_memory(&normalized).unwrap().width(), 30);
    }
}
