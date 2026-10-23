mod app;
mod domain;
mod provider;
mod runtime;
mod storage;
mod tools;
mod ui;

fn main() -> eframe::Result<()> {
    app::run()
}
