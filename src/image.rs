use crate::layout::{SpanStyle, StyledLine, StyledSpan};
use image::DynamicImage;
use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// Maximum image dimensions in character cells.
const MAX_IMAGE_WIDTH: u32 = 60;
const MAX_IMAGE_HEIGHT: u32 = 24; // Each row = 2 pixel rows with half-blocks

/// SVGs are vectors, so pick a rasterization size: scale the document up so
/// its longest side is about this many pixels (small SVGs stay crisp when the
/// terminal scales them), never below the document's natural size.
const SVG_RASTER_TARGET_PX: f32 = 1024.0;
/// Hard cap per side on the rasterized pixmap (memory bound: 4096² RGBA = 64 MiB).
const SVG_RASTER_MAX_PX: u32 = 4096;

/// Cache of decoded images so a resize / theme-change / watch reload re-uses
/// the decode instead of reading and decoding the file again. Keyed by a
/// stable identity: canonical path + mtime for local files, URL for remote.
type ImageCache = HashMap<String, Arc<DynamicImage>>;

fn cache() -> &'static Mutex<ImageCache> {
    static CACHE: OnceLock<Mutex<ImageCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Load and decode an image (via the cache), ready for half-block rendering.
pub fn load_decoded(
    src: &str,
    base_dir: Option<&Path>,
    mode: ImageMode,
) -> Result<Arc<DynamicImage>, ImageUnavailable> {
    let (key, bytes, resource_dir) = load_bytes_with_key(src, base_dir, mode)?;
    if let Some(img) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return Ok(img);
    }
    let decoded = if is_svg(src, &bytes) {
        Arc::new(rasterize_svg(&bytes, resource_dir.as_deref()).ok_or(ImageUnavailable::Failed)?)
    } else {
        Arc::new(decode_raster(&bytes).ok_or(ImageUnavailable::Failed)?)
    };
    if let Ok(mut c) = cache().lock() {
        c.insert(key, decoded.clone());
    }
    Ok(decoded)
}

/// Decode a raster image, honoring EXIF orientation — phone photos carry
/// their rotation as metadata, and ignoring it renders them sideways.
fn decode_raster(bytes: &[u8]) -> Option<DynamicImage> {
    use image::ImageDecoder;
    let reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?;
    let mut decoder = reader.into_decoder().ok()?;
    let orientation = decoder.orientation().ok();
    let mut img = DynamicImage::from_decoder(decoder).ok()?;
    if let Some(o) = orientation {
        img.apply_orientation(o);
    }
    Some(img)
}

/// Is this an SVG? The `image` crate only decodes raster formats, so SVGs
/// (and gzipped `.svgz`) are routed to the resvg rasterizer instead. Checked
/// by extension first, then by sniffing for an `<svg` tag in the header —
/// covers URLs and files with the wrong/no extension.
fn is_svg(src: &str, bytes: &[u8]) -> bool {
    let path = src.split(['?', '#']).next().unwrap_or(src);
    if let Some(ext) = path.rsplit('.').next() {
        if ext.eq_ignore_ascii_case("svg") || ext.eq_ignore_ascii_case("svgz") {
            return true;
        }
    }
    let head = &bytes[..bytes.len().min(1024)];
    head.windows(4).any(|w| w.eq_ignore_ascii_case(b"<svg"))
}

/// Shared font database for SVG text rendering. System-font discovery is
/// expensive (walks font directories), so do it once per process.
fn svg_fontdb() -> Arc<resvg::usvg::fontdb::Database> {
    static FONTDB: OnceLock<Arc<resvg::usvg::fontdb::Database>> = OnceLock::new();
    FONTDB
        .get_or_init(|| {
            let mut db = resvg::usvg::fontdb::Database::new();
            db.load_system_fonts();
            Arc::new(db)
        })
        .clone()
}

/// Rasterize an SVG document to a pixel image via resvg. Returns `None` on
/// parse failure or degenerate dimensions. `resources_dir` anchors relative
/// `<image href>` references (for local files, the SVG's own directory).
fn rasterize_svg(bytes: &[u8], resources_dir: Option<&Path>) -> Option<DynamicImage> {
    use resvg::{tiny_skia, usvg};

    let opt = usvg::Options {
        fontdb: svg_fontdb(),
        resources_dir: resources_dir.map(Path::to_path_buf),
        ..usvg::Options::default()
    };
    let tree = usvg::Tree::from_data(bytes, &opt).ok()?;
    let size = tree.size();
    if size.width() <= 0.0 || size.height() <= 0.0 {
        return None;
    }
    let scale = (SVG_RASTER_TARGET_PX / size.width().max(size.height())).max(1.0);
    let w = ((size.width() * scale).round() as u32).clamp(1, SVG_RASTER_MAX_PX);
    let h = ((size.height() * scale).round() as u32).clamp(1, SVG_RASTER_MAX_PX);
    let mut pixmap = tiny_skia::Pixmap::new(w, h)?;
    resvg::render(
        &tree,
        tiny_skia::Transform::from_scale(scale, scale),
        &mut pixmap.as_mut(),
    );

    // tiny-skia pixels are premultiplied RGBA; the image crate expects straight.
    let mut rgba = Vec::with_capacity((w as usize) * (h as usize) * 4);
    for p in pixmap.pixels() {
        let c = p.demultiply();
        rgba.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    Some(DynamicImage::ImageRgba8(image::RgbaImage::from_raw(
        w, h, rgba,
    )?))
}

/// Resolve `src`, returning a cache key, the raw bytes to decode, and (for
/// local files) the directory that anchors the file's own relative resources.
fn load_bytes_with_key(
    src: &str,
    base_dir: Option<&Path>,
    mode: ImageMode,
) -> Result<(String, Vec<u8>, Option<std::path::PathBuf>), ImageUnavailable> {
    if src.starts_with("http://") || src.starts_with("https://") {
        if mode != ImageMode::All {
            return Err(ImageUnavailable::RemoteBlocked);
        }
        let bytes = crate::net::fetch_untrusted_bytes(src, crate::net::IMAGE_FETCH_CAP)
            .map_err(|_| ImageUnavailable::Failed)?;
        return Ok((format!("url:{src}"), bytes, None));
    }
    // Embedded `data:` URIs (notebook/HTML exports inline images this way).
    if let Some(rest) = src.strip_prefix("data:") {
        return decode_data_uri(rest).ok_or(ImageUnavailable::Failed);
    }
    let base = base_dir.unwrap_or_else(|| Path::new("."));
    // Any readable local path is allowed — absolute, relative escaping the
    // document's directory, or a `file://` URL (issue #3: real documents say
    // `![](/tmp/chart.svg)`, and kitty's own icat displays them). This is
    // display-only and safe: the bytes must decode as an image and only ever
    // reach the screen as pixels, so a hostile document cannot surface file
    // *contents* as text, and remote fetches (the only exfiltration channel)
    // stay opt-in via --remote-images.
    let src = src
        .strip_prefix("file://")
        .map(|rest| rest.strip_prefix("localhost").unwrap_or(rest))
        .unwrap_or(src);
    let path = crate::sanitize::resolve_local(base, src).ok_or(ImageUnavailable::NotFound)?;
    let meta = std::fs::metadata(&path).map_err(|_| ImageUnavailable::NotFound)?;
    if !meta.is_file() {
        return Err(ImageUnavailable::NotFound);
    }
    if meta.len() > crate::net::IMAGE_FETCH_CAP {
        return Err(ImageUnavailable::Failed);
    }
    let mtime = meta
        .modified()
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let key = format!("file:{}:{mtime}", path.display());
    let bytes = std::fs::read(&path).map_err(|_| ImageUnavailable::NotFound)?;
    let resource_dir = path.parent().map(Path::to_path_buf);
    Ok((key, bytes, resource_dir))
}

/// Decode the remainder of a `data:` URI (`[<mediatype>][;base64],<payload>`).
/// Returns the cache key and bytes; the key hashes the content because the
/// URI text itself can be megabytes.
fn decode_data_uri(rest: &str) -> Option<(String, Vec<u8>, Option<std::path::PathBuf>)> {
    use base64::Engine;
    let (meta, payload) = rest.split_once(',')?;
    let bytes = if meta.to_ascii_lowercase().ends_with(";base64") {
        base64::engine::general_purpose::STANDARD
            .decode(payload.trim().as_bytes())
            .ok()?
    } else {
        crate::sanitize::percent_decode(payload)
            .map(String::into_bytes)
            .unwrap_or_else(|| payload.as_bytes().to_vec())
    };
    if bytes.len() as u64 > crate::net::IMAGE_FETCH_CAP {
        return None;
    }
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    bytes.hash(&mut hasher);
    Some((
        format!("data:{:x}:{}", hasher.finish(), bytes.len()),
        bytes,
        None,
    ))
}

/// Which images a document is allowed to load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageMode {
    /// No image rendering at all (`--no-images`).
    Off,
    /// Local files only — remote URLs show a placeholder (default; remote
    /// fetches from inside untrusted documents are SSRF/tracking vectors).
    LocalOnly,
    /// Local files and remote URLs (`--remote-images`).
    All,
}

/// Why an image could not be loaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImageUnavailable {
    /// Remote image while remote fetching is disabled.
    RemoteBlocked,
    /// The local path does not exist or is not a readable file.
    NotFound,
    /// Oversized, network, or decode failure.
    Failed,
}

/// Load image data from a file path or URL.
///
/// Relative paths resolve against `base_dir`; absolute paths load as-is.
/// The bytes are only ever rendered as pixels, never as text, so reading
/// any local path is safe. Local reads and remote fetches are both
/// size-capped.
pub fn load_image(
    src: &str,
    base_dir: Option<&Path>,
    mode: ImageMode,
) -> Result<Vec<u8>, ImageUnavailable> {
    load_bytes_with_key(src, base_dir, mode).map(|(_, bytes, _)| bytes)
}

/// Render a decoded image to styled lines using Unicode half-block characters (▄).
///
/// This rendering method works in **any** terminal that supports 24-bit (true) color.
/// Each character cell represents 2 vertical pixels: the top pixel uses the background
/// color and the bottom pixel uses the foreground color of the `▄` character.
///
/// Returns `None` if the image has zero dimensions.
pub fn render_halfblock(
    img: &DynamicImage,
    max_width: usize,
    margin: usize,
) -> Option<Vec<StyledLine>> {
    use image::GenericImageView;

    let (w, h) = img.dimensions();

    if w == 0 || h == 0 {
        return None;
    }

    // Calculate target dimensions that fit within constraints
    let target_w = (max_width as u32).min(MAX_IMAGE_WIDTH).min(w);
    let scale = target_w as f64 / w as f64;
    let mut target_h = ((h as f64 * scale) as u32).min(MAX_IMAGE_HEIGHT * 2);

    // Make height even for half-block pairing
    if target_h % 2 == 1 {
        target_h += 1;
    }
    if target_h == 0 {
        target_h = 2;
    }

    let resized = img.resize_exact(target_w, target_h, image::imageops::FilterType::Triangle);

    let margin_str = " ".repeat(margin);
    let mut lines = Vec::new();

    // Blank line before image
    lines.push(StyledLine::empty());

    for y in (0..target_h).step_by(2) {
        let mut line = StyledLine::new();

        if margin > 0 {
            line.push(StyledSpan {
                text: margin_str.clone(),
                style: SpanStyle::default(),
            });
        }

        for x in 0..target_w {
            let top = resized.get_pixel(x, y);
            let bottom = if y + 1 < target_h {
                resized.get_pixel(x, y + 1)
            } else {
                image::Rgba([0, 0, 0, 255])
            };

            line.push(StyledSpan {
                text: "▄".to_string(),
                style: SpanStyle {
                    fg: Some(format!(
                        "#{:02x}{:02x}{:02x}",
                        bottom[0], bottom[1], bottom[2]
                    )),
                    bg: Some(format!("#{:02x}{:02x}{:02x}", top[0], top[1], top[2])),
                    ..Default::default()
                },
            });
        }

        lines.push(line);
    }

    // Blank line after image
    lines.push(StyledLine::empty());

    Some(lines)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::GenericImageView;

    // The SVG from issue #3 (https://commons.wikimedia.org/wiki/File:Svg_example1.svg).
    const SAMPLE_SVG: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE svg PUBLIC "-//W3C//DTD SVG 1.1//EN"
  "http://www.w3.org/Graphics/SVG/1.1/DTD/svg11.dtd">
<svg xmlns="http://www.w3.org/2000/svg" version="1.1"
     width="120" height="120">
  <rect x="14" y="23" width="200" height="50" fill="lime"
      stroke="black" />
</svg>"#;

    #[test]
    fn svg_detected_by_extension_and_content() {
        assert!(is_svg("diagram.svg", b""));
        assert!(is_svg("diagram.SVG", b""));
        assert!(is_svg("diagram.svgz", b""));
        assert!(is_svg("https://x.test/a.svg?v=2#frag", b""));
        // Wrong extension, sniffed from content.
        assert!(is_svg("logo.xml", SAMPLE_SVG.as_bytes()));
        assert!(!is_svg("photo.png", &[0x89, b'P', b'N', b'G']));
    }

    #[test]
    fn svg_rasterizes_scaled_up_with_lime_rect() {
        let img = rasterize_svg(SAMPLE_SVG.as_bytes(), None).expect("svg should rasterize");
        // 120x120 doc scaled up to the raster target, aspect preserved.
        let (w, h) = img.dimensions();
        assert_eq!((w, h), (1024, 1024));
        // A pixel inside the rect (doc coords ~60,48 → scaled) is lime.
        let s = 1024.0 / 120.0;
        let p = img.get_pixel((60.0 * s) as u32, (48.0 * s) as u32);
        assert_eq!((p[0], p[1], p[2]), (0, 255, 0));
        // A pixel outside the rect is fully transparent.
        let p = img.get_pixel(5, 5);
        assert_eq!(p[3], 0);
    }

    #[test]
    fn svg_loads_end_to_end_through_load_decoded() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("pic.svg"), SAMPLE_SVG).unwrap();
        let img = load_decoded("pic.svg", Some(dir.path()), ImageMode::LocalOnly)
            .expect("load_decoded should rasterize svg");
        assert!(img.dimensions().0 > 0);
    }

    // Issue #3 reopened: the reporter's document references the SVG by
    // absolute path (`![](/tmp/sample_from_wikipedia.svg)`), which the old
    // containment check rejected before the decoder ever ran — so the SVG fix
    // appeared broken, and PNGs by absolute path failed the same way.
    #[test]
    fn absolute_path_outside_base_dir_loads() {
        let img_dir = tempfile::tempdir().unwrap();
        let doc_dir = tempfile::tempdir().unwrap();
        let svg_path = img_dir.path().join("sample_from_wikipedia.svg");
        std::fs::write(&svg_path, SAMPLE_SVG).unwrap();
        let img = load_decoded(
            svg_path.to_str().unwrap(),
            Some(doc_dir.path()),
            ImageMode::LocalOnly,
        )
        .expect("absolute image path should load");
        assert!(img.dimensions().0 > 0);
    }

    #[test]
    fn relative_path_escaping_base_dir_loads() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir(dir.path().join("docs")).unwrap();
        std::fs::write(dir.path().join("pic.svg"), SAMPLE_SVG).unwrap();
        let img = load_decoded(
            "../pic.svg",
            Some(&dir.path().join("docs")),
            ImageMode::LocalOnly,
        )
        .expect("parent-relative image path should load");
        assert!(img.dimensions().0 > 0);
    }

    #[test]
    fn percent_encoded_and_file_url_sources_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("my pic.svg"), SAMPLE_SVG).unwrap();
        // Markdown destinations percent-encode spaces.
        assert!(load_decoded("my%20pic.svg", Some(dir.path()), ImageMode::LocalOnly).is_ok());
        // file:// URLs resolve as local paths (percent-encoded, like real file URLs).
        let url = format!("file://{}/my%20pic.svg", dir.path().display());
        assert!(load_decoded(&url, Some(dir.path()), ImageMode::LocalOnly).is_ok());
        // Missing files report NotFound, not a generic failure.
        assert_eq!(
            load_decoded("nope.svg", Some(dir.path()), ImageMode::LocalOnly).err(),
            Some(ImageUnavailable::NotFound)
        );
    }

    #[test]
    fn data_uri_images_load() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(SAMPLE_SVG);
        let uri = format!("data:image/svg+xml;base64,{b64}");
        let img = load_decoded(&uri, None, ImageMode::LocalOnly).expect("base64 data uri");
        assert!(img.dimensions().0 > 0);
        // Percent-encoded (non-base64) payloads work too.
        let encoded: String = SAMPLE_SVG
            .chars()
            .map(|c| match c {
                '#' => "%23".to_string(),
                '\n' => "%0A".to_string(),
                c => c.to_string(),
            })
            .collect();
        let uri = format!("data:image/svg+xml,{encoded}");
        let img = load_decoded(&uri, None, ImageMode::LocalOnly).expect("plain data uri");
        assert!(img.dimensions().0 > 0);
        // Garbage payloads fail cleanly.
        assert_eq!(
            load_decoded("data:image/png;base64,!!!", None, ImageMode::LocalOnly).err(),
            Some(ImageUnavailable::Failed)
        );
    }

    #[test]
    fn raster_decode_still_works_through_orientation_path() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("dot.png");
        image::RgbaImage::from_pixel(3, 2, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        let img = load_decoded(
            path.to_str().unwrap(),
            Some(dir.path()),
            ImageMode::LocalOnly,
        )
        .expect("png decodes");
        assert_eq!(img.dimensions(), (3, 2));
    }

    #[test]
    fn svg_with_embedded_raster_image_renders() {
        let dir = tempfile::tempdir().unwrap();
        image::RgbaImage::from_pixel(8, 8, image::Rgba([255, 0, 0, 255]))
            .save(dir.path().join("side.png"))
            .unwrap();
        let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="8" height="8">
  <image href="side.png" x="0" y="0" width="8" height="8"/>
</svg>"#;
        std::fs::write(dir.path().join("embed.svg"), svg).unwrap();
        let img = load_decoded("embed.svg", Some(dir.path()), ImageMode::LocalOnly)
            .expect("svg with embedded raster");
        // The referenced PNG's red pixels must appear — blank output means the
        // raster-images feature or resources_dir anchoring regressed.
        let (w, h) = img.dimensions();
        let p = img.get_pixel(w / 2, h / 2);
        assert_eq!(
            (p[0], p[3]),
            (255, 255),
            "center pixel should be opaque red"
        );
    }

    #[test]
    fn invalid_svg_fails_cleanly() {
        assert!(rasterize_svg(b"<svg not really", None).is_none());
        assert!(rasterize_svg(b"", None).is_none());
    }
}
