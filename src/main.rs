fn main() {
    env_logger::init();

    if let Err(err) = revoxelation::app::run() {
        eprintln!("{err:#}");
        std::process::exit(1);
    }
}
