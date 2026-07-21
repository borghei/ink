fn main() {
    if let Err(e) = ink_md::cli::run() {
        eprintln!("ink: {e:#}");
        std::process::exit(1);
    }
}
