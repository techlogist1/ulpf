fn main() {
    if let Err(e) = ulpf::cli::main() {
        eprintln!("ulpf: {e:#}");
        std::process::exit(2);
    }
}
