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
/// stable identity computed *without touching content* (metadata at most):
/// canonical path + mtime for local files, URL for remote, a hash of the URI
/// text for `data:` sources. Failures are cached too — otherwise every
/// relayout would retry each doomed load (with `--remote-images`, a fresh
/// blocking network fetch per failed image per resize tick). A fixed local
/// file busts its negative entry naturally via the mtime in the key; remote
/// failures persist for the session (deliberate: each retry would block the
/// render loop on the network again).
type CachedLoad = Result<Arc<DynamicImage>, ImageUnavailable>;
type ImageCache = HashMap<String, CachedLoad>;

/// Bound on cached entries. When an insert would exceed it, the whole map is
/// cleared first — crude generational eviction. Watch-mode mtime changes mint
/// new keys and strand old decodes (an SVG entry can run ~4 MB), so some
/// bound is needed, but an LRU is overkill for a TUI viewing a handful of
/// documents: re-decoding one screenful after a rare purge is cheap.
const IMAGE_CACHE_CAP: usize = 64;

fn cache() -> &'static Mutex<ImageCache> {
    static CACHE: OnceLock<Mutex<ImageCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

/// Test-only view of the cache size, for the eviction regression test.
#[cfg(test)]
fn cache_len() -> usize {
    cache().lock().map(|c| c.len()).unwrap_or(0)
}

/// Load and decode an image (via the cache), ready for half-block rendering.
///
/// `svg_color` is the theme's foreground color: SVG `currentColor` resolves
/// to it, so monochrome icons (octicons, simple-icons badges) follow the
/// theme instead of defaulting to black — invisible on dark terminals.
pub fn load_decoded(
    src: &str,
    base_dir: Option<&Path>,
    mode: ImageMode,
    svg_color: Option<&str>,
) -> Result<Arc<DynamicImage>, ImageUnavailable> {
    // Resolve the key first — no content reads, no network — so the cache
    // answers *before* any I/O. A relayout must not re-read every image file,
    // let alone re-download remote ones.
    let (mut key, source) = resolve_source(src, base_dir, mode)?;
    // The rasterization bakes currentColor in, so a theme change must miss
    // the cache and re-rasterize. Extension-detected SVGs only: no bytes
    // exist yet to sniff, so an extension-less SVG keeps its first theme's
    // color until the entry is evicted — a cosmetic, rare trade for never
    // touching content before the cache.
    if let (Some(color), true) = (svg_color, is_svg(src, b"")) {
        key = format!("{key}|color:{color}");
    }
    if let Some(cached) = cache().lock().ok().and_then(|c| c.get(&key).cloned()) {
        return cached;
    }
    let result = fetch_and_decode(src, &source, svg_color);
    if let Ok(mut c) = cache().lock() {
        if c.len() >= IMAGE_CACHE_CAP && !c.contains_key(&key) {
            c.clear(); // generational eviction — see IMAGE_CACHE_CAP
        }
        c.insert(key, result.clone());
    }
    result
}

/// The expensive path — read/fetch the bytes and decode them. Runs only on a
/// cache miss; the outcome (success or failure) is what gets cached.
fn fetch_and_decode(src: &str, source: &ImageSource<'_>, svg_color: Option<&str>) -> CachedLoad {
    let (bytes, resource_dir) = fetch_bytes(source)?;
    let decoded = if is_svg(src, &bytes) {
        rasterize_svg(&bytes, resource_dir.as_deref(), svg_color)
    } else {
        decode_raster(&bytes)
    };
    decoded.map(Arc::new).ok_or(ImageUnavailable::Failed)
}

/// `ink doctor` self-test: does the SVG rasterizer work on this build?
pub fn self_test_rasterize(svg: &[u8]) -> bool {
    rasterize_svg(svg, None, None).is_some()
}

/// `ink doctor` self-test: does the raster decoder work on this build?
pub fn self_test_decode(bytes: &[u8]) -> bool {
    decode_raster(bytes).is_some()
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
/// `<image href>` references (for local files, the SVG's own directory);
/// `current_color` becomes the document's `color`, so `currentColor` fills
/// and strokes follow the theme.
fn rasterize_svg(
    bytes: &[u8],
    resources_dir: Option<&Path>,
    current_color: Option<&str>,
) -> Option<DynamicImage> {
    use resvg::{tiny_skia, usvg};

    // Gzipped .svgz must be decompressed before the color injection can see
    // the markup (usvg would gunzip internally, but too late for us).
    let gunzipped = if bytes.starts_with(&[0x1f, 0x8b]) {
        gunzip_capped(bytes)
    } else {
        None
    };
    let bytes = gunzipped.as_deref().unwrap_or(bytes);
    let injected = current_color.and_then(|c| inject_current_color(bytes, c));
    let bytes = injected.as_deref().unwrap_or(bytes);
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

/// Set the SVG root's `color` presentation attribute so `currentColor`
/// resolves to the theme foreground. Skipped when the document sets its own
/// `color`, when the color string isn't a plain hex value (it is spliced
/// into markup), or for non-UTF-8 input (gzipped `.svgz`).
fn inject_current_color(bytes: &[u8], color: &str) -> Option<Vec<u8>> {
    if !color.chars().all(|c| c == '#' || c.is_ascii_hexdigit()) {
        return None;
    }
    let text = std::str::from_utf8(bytes).ok()?;
    let start = text.find("<svg")?;
    let end = start + svg_root_tag_end(&text[start..])?;
    // Conservative: any `color=`-ish content in the root tag (including
    // style="color:…") means the author chose, so leave it alone.
    if text[start..end].contains("color") {
        return None;
    }
    let insert_at = if text[..end].ends_with('/') {
        end - 1
    } else {
        end
    };
    let mut out = String::with_capacity(text.len() + 24);
    out.push_str(&text[..insert_at]);
    out.push_str(&format!(" color=\"{color}\""));
    out.push_str(&text[insert_at..]);
    Some(out.into_bytes())
}

/// Decompress gzip input with a hard output cap (decompression-bomb guard:
/// a tiny .svgz must not expand into gigabytes).
fn gunzip_capped(bytes: &[u8]) -> Option<Vec<u8>> {
    use std::io::Read;
    const GUNZIP_CAP: u64 = 64 * 1024 * 1024;
    let mut out = Vec::new();
    let mut reader = flate2::read::GzDecoder::new(bytes).take(GUNZIP_CAP + 1);
    reader.read_to_end(&mut out).ok()?;
    (out.len() as u64 <= GUNZIP_CAP).then_some(out)
}

/// Index of the root tag's closing `>` — quote-aware, so a `>` inside a
/// quoted attribute value doesn't truncate the tag.
fn svg_root_tag_end(tag: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in tag.char_indices() {
        match (quote, c) {
            (Some(q), _) if c == q => quote = None,
            (None, '"') | (None, '\'') => quote = Some(c),
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

/// A resolved image source: where the bytes will come from on a cache miss.
enum ImageSource<'a> {
    /// Remote URL (only reachable in `ImageMode::All`).
    Remote(&'a str),
    /// The remainder of a `data:` URI (after the scheme prefix).
    DataUri(&'a str),
    /// A verified regular local file (canonicalized).
    Local(std::path::PathBuf),
}

/// Resolve `src` into a cache key and a fetchable source, without reading any
/// content: the key comes from the URL, from the `data:` URI text, or (for
/// local files) from canonical path + mtime via `fs::metadata` only. Cheap
/// failures detected here (blocked remote, missing file, oversize) are
/// returned directly and not cached.
fn resolve_source<'a>(
    src: &'a str,
    base_dir: Option<&Path>,
    mode: ImageMode,
) -> Result<(String, ImageSource<'a>), ImageUnavailable> {
    if src.starts_with("http://") || src.starts_with("https://") {
        if mode != ImageMode::All {
            return Err(ImageUnavailable::RemoteBlocked);
        }
        return Ok((format!("url:{src}"), ImageSource::Remote(src)));
    }
    // Embedded `data:` URIs (notebook/HTML exports inline images this way).
    // The key hashes the URI text because it can be megabytes — but never the
    // decoded bytes: decoding is part of the work the cache exists to skip.
    if let Some(rest) = src.strip_prefix("data:") {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        src.hash(&mut hasher);
        let key = format!("data:{:x}:{}", hasher.finish(), src.len());
        return Ok((key, ImageSource::DataUri(rest)));
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
    Ok((key, ImageSource::Local(path)))
}

/// Read a source's bytes — file read, network fetch, or data-URI decode.
/// This is the I/O the cache skips; it runs only on a miss. Also returns the
/// directory anchoring a local file's own relative resources (SVG `<image
/// href>`).
fn fetch_bytes(
    source: &ImageSource<'_>,
) -> Result<(Vec<u8>, Option<std::path::PathBuf>), ImageUnavailable> {
    match source {
        ImageSource::Remote(url) => {
            let bytes = crate::net::fetch_untrusted_bytes(url, crate::net::IMAGE_FETCH_CAP)
                .map_err(|_| ImageUnavailable::Failed)?;
            Ok((bytes, None))
        }
        ImageSource::DataUri(rest) => {
            let bytes = decode_data_uri(rest).ok_or(ImageUnavailable::Failed)?;
            Ok((bytes, None))
        }
        ImageSource::Local(path) => {
            let bytes = std::fs::read(path).map_err(|_| ImageUnavailable::NotFound)?;
            Ok((bytes, path.parent().map(Path::to_path_buf)))
        }
    }
}

/// Byte-level `%XX` decoding for data-URI payloads — unlike path decoding,
/// the result may legitimately be binary (a percent-encoded PNG), so this
/// never round-trips through UTF-8. Malformed escapes pass through.
fn percent_decode_bytes(s: &str) -> Vec<u8> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = |b: u8| (b as char).to_digit(16).map(|d| d as u8);
            if let (Some(hi), Some(lo)) = (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                out.push(hi * 16 + lo);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    out
}

/// Decode the remainder of a `data:` URI (`[<mediatype>][;base64],<payload>`).
fn decode_data_uri(rest: &str) -> Option<Vec<u8>> {
    use base64::Engine;
    let (meta, payload) = rest.split_once(',')?;
    let bytes = if meta.to_ascii_lowercase().ends_with(";base64") {
        base64::engine::general_purpose::STANDARD
            .decode(payload.trim().as_bytes())
            .ok()?
    } else {
        percent_decode_bytes(payload)
    };
    (bytes.len() as u64 <= crate::net::IMAGE_FETCH_CAP).then_some(bytes)
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
    let (_, source) = resolve_source(src, base_dir, mode)?;
    fetch_bytes(&source).map(|(bytes, _)| bytes)
}

/// Render a decoded image to styled lines using Unicode half-block characters (▄).
///
/// This rendering method works in **any** terminal that supports 24-bit (true) color.
/// Each character cell represents 2 vertical pixels: the top pixel uses the background
/// color and the bottom pixel uses the foreground color of the `▄` character.
///
/// Cells are opaque, so transparency must be composited here: each pixel is
/// alpha-blended over `bg` (the theme background). Without this, transparent
/// regions — most SVGs, logos, badges — render as solid black boxes.
///
/// Returns `None` if the image has zero dimensions.
pub fn render_halfblock(
    img: &DynamicImage,
    max_width: usize,
    margin: usize,
    bg: (u8, u8, u8),
) -> Option<Vec<StyledLine>> {
    use image::GenericImageView;

    let blend = |p: image::Rgba<u8>| -> (u8, u8, u8) {
        let a = p[3] as u32;
        let over = |c: u8, b: u8| ((c as u32 * a + b as u32 * (255 - a)) / 255) as u8;
        (over(p[0], bg.0), over(p[1], bg.1), over(p[2], bg.2))
    };

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
            let top = blend(resized.get_pixel(x, y));
            let bottom = if y + 1 < target_h {
                blend(resized.get_pixel(x, y + 1))
            } else {
                // Padding row past the image edge: pure background.
                bg
            };

            line.push(StyledSpan {
                text: "▄".to_string(),
                style: SpanStyle {
                    fg: Some(format!("#{:02x}{:02x}{:02x}", bottom.0, bottom.1, bottom.2)),
                    bg: Some(format!("#{:02x}{:02x}{:02x}", top.0, top.1, top.2)),
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
        let img = rasterize_svg(SAMPLE_SVG.as_bytes(), None, None).expect("svg should rasterize");
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
        let img = load_decoded("pic.svg", Some(dir.path()), ImageMode::LocalOnly, None)
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
            None,
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
            None,
        )
        .expect("parent-relative image path should load");
        assert!(img.dimensions().0 > 0);
    }

    #[test]
    fn percent_encoded_and_file_url_sources_load() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("my pic.svg"), SAMPLE_SVG).unwrap();
        // Markdown destinations percent-encode spaces.
        assert!(load_decoded("my%20pic.svg", Some(dir.path()), ImageMode::LocalOnly, None).is_ok());
        // file:// URLs resolve as local paths (percent-encoded, like real file URLs).
        let url = format!("file://{}/my%20pic.svg", dir.path().display());
        assert!(load_decoded(&url, Some(dir.path()), ImageMode::LocalOnly, None).is_ok());
        // Missing files report NotFound, not a generic failure.
        assert_eq!(
            load_decoded("nope.svg", Some(dir.path()), ImageMode::LocalOnly, None).err(),
            Some(ImageUnavailable::NotFound)
        );
    }

    #[test]
    fn data_uri_images_load() {
        use base64::Engine;
        let b64 = base64::engine::general_purpose::STANDARD.encode(SAMPLE_SVG);
        let uri = format!("data:image/svg+xml;base64,{b64}");
        let img = load_decoded(&uri, None, ImageMode::LocalOnly, None).expect("base64 data uri");
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
        let img = load_decoded(&uri, None, ImageMode::LocalOnly, None).expect("plain data uri");
        assert!(img.dimensions().0 > 0);
        // Garbage payloads fail cleanly.
        assert_eq!(
            load_decoded(
                "data:image/png;base64,!!!",
                None,
                ImageMode::LocalOnly,
                None
            )
            .err(),
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
            None,
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
        let img = load_decoded("embed.svg", Some(dir.path()), ImageMode::LocalOnly, None)
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
        assert!(rasterize_svg(b"<svg not really", None, None).is_none());
        assert!(rasterize_svg(b"", None, None).is_none());
    }

    /// Tests below assert on cross-call cache state, and the cache is one
    /// process-global map shared by every parallel test thread. Serialize
    /// them so the eviction test's clear cannot race a hit assertion.
    static CACHE_TEST_LOCK: Mutex<()> = Mutex::new(());

    fn lock_cache_tests() -> std::sync::MutexGuard<'static, ()> {
        CACHE_TEST_LOCK
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Restore a file's mtime so its cache key (canonical path + mtime) is
    /// byte-identical to an earlier load's.
    fn set_mtime(path: &Path, mtime: SystemTime) {
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
    }

    // Regression: the cache used to be consulted only AFTER the file was
    // read, so every relayout re-read every image from disk. Corrupting the
    // file while keeping its mtime (same key) proves a hit does no I/O.
    #[test]
    fn cache_hit_serves_decode_without_rereading_the_file() {
        let _guard = lock_cache_tests();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("cached.png");
        image::RgbaImage::from_pixel(2, 2, image::Rgba([0, 128, 255, 255]))
            .save(&path)
            .unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let first = load_decoded(path.to_str().unwrap(), None, ImageMode::LocalOnly, None)
            .expect("first load decodes");
        std::fs::write(&path, b"GARBAGE, not an image").unwrap();
        set_mtime(&path, mtime);
        let second = load_decoded(path.to_str().unwrap(), None, ImageMode::LocalOnly, None)
            .expect("second load must be served from cache, not the garbage on disk");
        assert!(
            Arc::ptr_eq(&first, &second),
            "hit must reuse the same decode"
        );
    }

    // Regression: failures were never cached, so every relayout retried every
    // doomed load (a blocking network fetch per resize tick under
    // --remote-images). A failed decode must be cached under the same
    // path+mtime key — and a changed mtime must bust the negative entry.
    #[test]
    fn failed_decode_is_cached_until_the_file_changes() {
        let _guard = lock_cache_tests();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("broken.png");
        std::fs::write(&path, b"definitely not an image").unwrap();
        let mtime = std::fs::metadata(&path).unwrap().modified().unwrap();
        let src = path.to_str().unwrap();
        assert_eq!(
            load_decoded(src, None, ImageMode::LocalOnly, None).err(),
            Some(ImageUnavailable::Failed)
        );
        // Fix the file but keep the mtime: the identical key must serve the
        // cached failure — proof the retry did no fresh read or decode.
        image::RgbaImage::from_pixel(2, 2, image::Rgba([255, 0, 0, 255]))
            .save(&path)
            .unwrap();
        set_mtime(&path, mtime);
        assert_eq!(
            load_decoded(src, None, ImageMode::LocalOnly, None).err(),
            Some(ImageUnavailable::Failed)
        );
        // Bump the mtime: new key, and the fixed file loads for real.
        set_mtime(&path, mtime + std::time::Duration::from_secs(2));
        assert!(load_decoded(src, None, ImageMode::LocalOnly, None).is_ok());
    }

    #[test]
    fn missing_file_reports_not_found_consistently() {
        let dir = tempfile::tempdir().unwrap();
        let src = dir.path().join("ghost.png");
        let src = src.to_str().unwrap();
        for _ in 0..2 {
            assert_eq!(
                load_decoded(src, None, ImageMode::LocalOnly, None).err(),
                Some(ImageUnavailable::NotFound)
            );
        }
    }

    // Regression: the cache had no bound, so watch reloads and long sessions
    // grew it without limit. Loading more distinct images than the cap must
    // leave the map at or under the cap.
    #[test]
    fn cache_stays_bounded_under_many_distinct_images() {
        let _guard = lock_cache_tests();
        for i in 0..(IMAGE_CACHE_CAP + 8) {
            // Distinct width per iteration → distinct data-URI text → key.
            let svg = format!(
                r#"<svg xmlns="http://www.w3.org/2000/svg" width="{}" height="1"></svg>"#,
                2048 + i
            );
            let uri = format!("data:image/svg+xml,{svg}");
            assert!(load_decoded(&uri, None, ImageMode::LocalOnly, None).is_ok());
            assert!(cache_len() <= IMAGE_CACHE_CAP, "cache exceeded its cap");
        }
        assert!(cache_len() > 0, "eviction must not leave the cache useless");
    }
}
