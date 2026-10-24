mod app;
mod domain;
mod provider;
mod runtime;
mod storage;
mod tools;
mod ui;
mod web;

fn main() -> eframe::Result<()> {
    let report = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        storage::log_crash(&format!(
            "panic in {}: {info}",
            thread.name().unwrap_or("unnamed thread")
        ));
        report(info);
    }));
    app::run()
}
