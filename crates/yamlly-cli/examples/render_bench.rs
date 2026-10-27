use ratatui::{Terminal, backend::TestBackend};
use std::time::Instant;

use yamlly::app::App;
use yamlly::ui;

fn main() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    for _ in 0..6 {
        app.open_selected_resource();
        app.select_next_resource();
    }
    app.select_next_resource();

    let backend = TestBackend::new(120, 40);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::render(&mut ratcn, f, &app)).unwrap();

    let iters = 2000;
    let start = Instant::now();
    for _ in 0..iters {
        term.draw(|f| ui::render(&mut ratcn, f, &app)).unwrap();
    }
    let el = start.elapsed();
    println!(
        "ui::render full frame          {:>8.4} ms/frame   -> {:.1} fps ceiling",
        el.as_secs_f64() * 1000.0 / iters as f64,
        iters as f64 / el.as_secs_f64()
    );
}
