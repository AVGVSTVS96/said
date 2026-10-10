fn main() {
    if let Err(error) = voice_bench::cli(&std::env::args().skip(1).collect::<Vec<_>>()) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
