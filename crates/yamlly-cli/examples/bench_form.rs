use std::time::Instant;
use yamlly::form::FormState;
fn main() {
    // Form build cost per kind (rebuild does a full schema walk).
    for kind in ["Pod", "Deployment", "CronJob"] {
        let iters = 200;
        let mut f = FormState::new(kind, None, vec![]);
        let s = Instant::now();
        for _ in 0..iters {
            f.rebuild();
            std::hint::black_box(&f.rows);
        }
        println!(
            "FormState::rebuild({kind:<11}) {:>8.4} ms   rows={}",
            s.elapsed().as_secs_f64() * 1000.0 / iters as f64,
            f.rows.len()
        );
    }
    // Keystroke cost: begin_edit + insert + commit
    let mut f = FormState::new("Deployment", None, vec![]);
    let idx = f
        .rows
        .iter()
        .position(|r| matches!(&r.kind, yamlly_core::field::Row::Field { .. }))
        .unwrap();
    f.focus = idx;
    let iters = 100_000;
    let s = Instant::now();
    for _ in 0..iters {
        f.begin_edit();
        f.insert_char('x');
        f.commit();
    }
    println!(
        "keystroke (edit+commit)     {:>8.4} ms/iter",
        s.elapsed().as_secs_f64() * 1000.0 / iters as f64
    );
    let _ = std::hint::black_box(&f);
}
