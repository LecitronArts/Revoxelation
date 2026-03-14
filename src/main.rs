mod app;
mod ecs;
mod renderer;
mod world;

fn main() {
    env_logger::init();
    if let Err(err) = app::run() {
        eprintln!("fatal error: {err:#}");
    }
}
