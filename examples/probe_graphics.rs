//! Diagnostic: what graphics protocol does the terminal negotiation pick?
//! Run inside the terminal under test: `cargo run --example probe_graphics`
fn main() {
    match ratatui_image::picker::Picker::from_query_stdio() {
        Ok(p) => eprintln!(
            "protocol: {:?}, font: {}x{}",
            p.protocol_type(),
            p.font_size().width,
            p.font_size().height
        ),
        Err(e) => eprintln!("query failed: {e}"),
    }
}
