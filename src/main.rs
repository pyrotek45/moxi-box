mod backend;
mod core;
mod models;
mod storage;
mod tools;
mod ui;
mod widgets;

fn main() -> eframe::Result<()> {
    ui::run()
}
