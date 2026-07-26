//! SVG rendering regressions beyond decoding: transparency compositing and
//! theme-aware `currentColor` (both found by probing after issue #3).

use image::GenericImageView;
use ink_md::image::{load_decoded, render_halfblock, ImageMode};

fn load_svg(dir: &std::path::Path, name: &str, svg: &str) -> std::sync::Arc<image::DynamicImage> {
    std::fs::write(dir.join(name), svg).unwrap();
    load_decoded(name, Some(dir), ImageMode::LocalOnly, Some("#e0e0e0")).expect(name)
}

// Half-block cells are opaque, so transparent regions must composite over the
// theme background — not collapse to black (a black box on light themes).
#[test]
fn transparent_regions_composite_over_theme_background() {
    let dir = tempfile::tempdir().unwrap();
    let img = load_svg(
        dir.path(),
        "t.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100">
             <rect x="40" y="40" width="20" height="20" fill="lime"/></svg>"#,
    );
    // Light-theme backdrop: the fully transparent corner must be white.
    let lines = render_halfblock(&img, 40, 0, (255, 255, 255)).unwrap();
    let corner = &lines[1].spans[0];
    assert_eq!(corner.style.bg.as_deref(), Some("#ffffff"));
    assert_eq!(corner.style.fg.as_deref(), Some("#ffffff"));
    // Dark-theme backdrop: same corner is the dark color, not hardcoded black.
    let lines = render_halfblock(&img, 40, 0, (30, 30, 46)).unwrap();
    let corner = &lines[1].spans[0];
    assert_eq!(corner.style.bg.as_deref(), Some("#1e1e2e"));
}

// `currentColor` icons (octicons, simple-icons) must take the theme
// foreground instead of SVG's black default — invisible on dark terminals.
#[test]
fn current_color_resolves_to_theme_foreground() {
    let dir = tempfile::tempdir().unwrap();
    let img = load_svg(
        dir.path(),
        "c.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
             <rect width="10" height="10" fill="currentColor"/></svg>"#,
    );
    let (w, h) = img.dimensions();
    let p = img.get_pixel(w / 2, h / 2);
    assert_eq!(
        (p[0], p[1], p[2]),
        (0xe0, 0xe0, 0xe0),
        "theme fg, not black"
    );
}

// A document that sets its own `color` keeps it — the theme must not override.
#[test]
fn author_color_wins_over_theme() {
    let dir = tempfile::tempdir().unwrap();
    let img = load_svg(
        dir.path(),
        "own.svg",
        r##"<svg xmlns="http://www.w3.org/2000/svg" color="#ff0000" width="10" height="10">
             <rect width="10" height="10" fill="currentColor"/></svg>"##,
    );
    let (w, h) = img.dimensions();
    let p = img.get_pixel(w / 2, h / 2);
    assert_eq!((p[0], p[1], p[2]), (255, 0, 0), "author's color wins");
}

// SVGs embedding other SVGs keep working (guards the resvg feature set).
#[test]
fn svg_embedding_svg_renders() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("inner.svg"),
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
             <rect width="10" height="10" fill="red"/></svg>"#,
    )
    .unwrap();
    let img = load_svg(
        dir.path(),
        "outer.svg",
        r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
             <image href="inner.svg" width="10" height="10"/></svg>"#,
    );
    let (w, h) = img.dimensions();
    let p = img.get_pixel(w / 2, h / 2);
    assert_eq!((p[0], p[1], p[2], p[3]), (255, 0, 0, 255));
}

// Gzipped .svgz gets currentColor theming too (the injector needs to see the
// markup, so the gzip layer is decompressed first).
#[test]
fn svgz_gets_current_color_theming() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let svg = r#"<svg xmlns="http://www.w3.org/2000/svg" width="10" height="10">
         <rect width="10" height="10" fill="currentColor"/></svg>"#;
    let mut enc = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(svg.as_bytes()).unwrap();
    std::fs::write(dir.path().join("z.svgz"), enc.finish().unwrap()).unwrap();
    let img = load_decoded(
        "z.svgz",
        Some(dir.path()),
        ImageMode::LocalOnly,
        Some("#e0e0e0"),
    )
    .expect("svgz loads");
    let (w, h) = img.dimensions();
    let p = img.get_pixel(w / 2, h / 2);
    assert_eq!(
        (p[0], p[1], p[2]),
        (0xe0, 0xe0, 0xe0),
        "svgz follows theme fg"
    );
}

// Percent-encoded data URIs may carry binary payloads (a raw PNG), which are
// not valid UTF-8 — decoding must be byte-level.
#[test]
fn percent_encoded_binary_data_uri_loads() {
    let mut png = Vec::new();
    image::RgbaImage::from_pixel(2, 2, image::Rgba([9, 8, 7, 255]))
        .write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let encoded: String = png.iter().map(|b| format!("%{b:02X}")).collect();
    let uri = format!("data:image/png,{encoded}");
    let img = load_decoded(&uri, None, ImageMode::LocalOnly, None).expect("binary data uri");
    assert_eq!(img.dimensions(), (2, 2));
}
