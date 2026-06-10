fn main() {
    if let Err(err) = clserver::run() {
        eprintln!("Error: {err:#}");
        std::process::exit(1);
    }
}
