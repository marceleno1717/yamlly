use serde_json::{json, Value};
use yamlly_core::normalize;

// Original (pre-optimization) implementation, kept here as the oracle.
fn is_dirty_old(current: &Value, baseline: &Value) -> bool {
    let norm = normalize(current).unwrap_or(Value::Object(serde_json::Map::new()));
    &norm != baseline
}

// Deterministic xorshift so the corpus is reproducible.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

fn rand_scalar(r: &mut Rng) -> Value {
    match r.below(7) {
        0 => Value::Null,
        1 => json!(r.below(2) == 0),
        2 => json!(r.below(1000) as i64),
        3 => json!(r.below(1000) as f64 / 8.0),
        4 => json!(r.below(1000) as u64),
        5 => json!(""),
        _ => json!(match r.below(6) {
            0 => "s".to_string(),
            1 => String::new(),
            2 => "True".to_string(),
            3 => "0.0".to_string(),
            4 => "hello world".to_string(),
            _ => "x".repeat(r.below(40)),
        }),
    }
}

fn rand_value(r: &mut Rng, depth: usize) -> Value {
    if depth == 0 || r.below(3) == 0 {
        return rand_scalar(r);
    }
    if r.below(2) == 0 {
        let n = r.below(4);
        Value::Array((0..n).map(|_| rand_value(r, depth - 1)).collect())
    } else {
        let n = r.below(5);
        let mut m = serde_json::Map::new();
        for i in 0..n {
            m.insert(format!("k{i}"), rand_value(r, depth - 1));
        }
        Value::Object(m)
    }
}

#[test]
fn is_dirty_matches_original_implementation() {
    let mut r = Rng(0x2545F4914F6CDD1D);
    let mut mismatches = 0;
    let mut cases = 0;

    for _ in 0..40_000 {
        let cur = rand_value(&mut r, 4);
        let baseline = match r.below(3) {
            // baseline is normally a normalized snapshot of some document
            0 => normalize(&rand_value(&mut r, 4)).unwrap_or(Value::Object(Default::default())),
            // ...but exercise the "empty" canonical form too
            1 => Value::Object(Default::default()),
            // ...and an arbitrary non-normalized value
            _ => rand_value(&mut r, 3),
        };
        cases += 1;
        if is_dirty_old(&cur, &baseline) != yamlly_core::is_dirty(&cur, &baseline) {
            if mismatches < 5 {
                eprintln!(
                    "MISMATCH\n  current  = {cur}\n  baseline = {baseline}\n  old={} new={}",
                    is_dirty_old(&cur, &baseline),
                    yamlly_core::is_dirty(&cur, &baseline)
                );
            }
            mismatches += 1;
        }
    }

    assert_eq!(mismatches, 0, "{mismatches}/{cases} cases diverged");
    println!("is_dirty differential: {cases} cases, 0 mismatches");
}

#[test]
fn is_dirty_baseline_sanity() {
    let doc = json!({"a": "b", "c": [1, 2], "d": {"e": "f"}, "g": ""});
    let base = normalize(&doc).unwrap();
    assert!(!yamlly_core::is_dirty(&doc, &base));
    // empty string is dropped by normalize -> not dirty
    assert!(!yamlly_core::is_dirty(&doc, &base));
    // added key -> dirty
    let mut d2 = doc.clone();
    d2["h"] = json!("i");
    assert!(yamlly_core::is_dirty(&d2, &base));
    // changed value -> dirty
    let mut d3 = doc.clone();
    d3["a"] = json!("z");
    assert!(yamlly_core::is_dirty(&d3, &base));
    // removed key -> dirty
    let mut d4 = doc.clone();
    d4.as_object_mut().unwrap().remove("a");
    assert!(yamlly_core::is_dirty(&d4, &base));
    // all-empty document is not dirty vs empty baseline
    assert!(!yamlly_core::is_dirty(
        &json!({}),
        &Value::Object(Default::default())
    ));
    assert!(!yamlly_core::is_dirty(
        &Value::Null,
        &Value::Object(Default::default())
    ));
    assert!(!yamlly_core::is_dirty(
        &json!([]),
        &Value::Object(Default::default())
    ));
    assert!(!yamlly_core::is_dirty(
        &json!(""),
        &Value::Object(Default::default())
    ));
    // element added to array -> dirty
    let mut d5 = doc.clone();
    d5["c"].as_array_mut().unwrap().push(json!(3));
    assert!(yamlly_core::is_dirty(&d5, &base));
    // empty element appended is dropped by normalize -> NOT dirty
    let mut d6 = doc.clone();
    d6["c"].as_array_mut().unwrap().push(json!(""));
    assert!(!yamlly_core::is_dirty(&d6, &base));
}
