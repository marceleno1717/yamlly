use ratatui::{Terminal, backend::TestBackend};
use std::time::Instant;
use yamlly::app::App;
use yamlly::ui;

fn main() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    let backend = TestBackend::new(150, 45);
    let mut term = Terminal::new(backend).unwrap();
    term.draw(|f| ui::render(&mut ratcn, f, &app)).unwrap();

    let batch = 800;
    let mut best = f64::MAX;

    for _ in 0..7 {
        let s = Instant::now();
        for _ in 0..batch {
            term.draw(|f| ui::render(&mut ratcn, f, &app)).unwrap();
        }
        let ms = s.elapsed().as_secs_f64() * 1000.0 / batch as f64;
        if ms < best {
            best = ms;
        }
    }
    println!("ui::render (form open) best-of-7 {:>8.4} ms/frame", best);
}
