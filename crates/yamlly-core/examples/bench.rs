use std::time::Instant;
use yamlly_core::*;

fn time<T>(label: &str, iters: u32, mut f: impl FnMut() -> T) {
    // warmup
    for _ in 0..iters.min(3) {
        std::hint::black_box(f());
    }
    let start = Instant::now();
    for _ in 0..iters {
        std::hint::black_box(f());
    }
    let el = start.elapsed();
    println!(
        "{:<42} {:>9.3} ms total  {:>9.4} ms/iter",
        label,
        el.as_secs_f64() * 1000.0,
        el.as_secs_f64() * 1000.0 / iters as f64
    );
}

fn main() {
    // 1. Cold shard load (worst case: LazyLock init of one shard)
    time("load_resource(Pod) [warm clone]", 2000, || {
        load_resource("Pod").unwrap()
    });

    // 2. get_leaf_field_paths over the biggest schemas
    for kind in ["Pod", "Deployment", "CronJob", "StatefulSet"] {
        let fields = load_resource(kind).unwrap()["fields"].clone();
        time(
            &format!("get_leaf_field_paths({kind}) uncached"),
            200,
            || get_leaf_field_paths(&fields),
        );
    }
    for kind in ["Pod", "Deployment", "CronJob"] {
        time(
            &format!("get_leaf_field_paths_cached({kind})"),
            2_000_000,
            || get_leaf_field_paths_cached(kind).unwrap().len(),
        );
    }
    time("load_resource_ref(Pod) [no clone]", 5_000_000, || {
        load_resource_ref("Pod").unwrap()
    });

    // 3. is_dirty / normalize on a realistic manifest
    let manifest = load_resource("Deployment").unwrap();
    let baseline = normalize(&manifest).unwrap();
    time("is_dirty(Deployment schema)", 2000, || {
        is_dirty(&manifest, &baseline)
    });

    // 4. emit_yaml_from_value on a realistic manifest
    let doc = serde_json::json!({
        "kind": "Deployment", "apiVersion": "apps/v1",
        "metadata": {"name": "web", "namespace": "default",
                     "labels": {"app": "web", "app.kubernetes.io/name": "web"},
                     "annotations": {"description": "a fairly long annotation value that will need wrapping at 120 columns for sure"}},
        "spec": {"replicas": 3,
                 "selector": {"matchLabels": {"app": "web"}},
                 "template": {"metadata": {"labels": {"app": "web"}},
                     "spec": {"containers": [
                        {"name": "web", "image": "nginx:1.25",
                         "ports": [{"containerPort": 80, "name": "http"}],
                         "env": [{"name": "LOG_LEVEL", "value": "info"}, {"name": "TZ", "value": "UTC"}],
                         "resources": {"requests": {"cpu": "100m", "memory": "128Mi"},
                                       "limits": {"cpu": "500m", "memory": "512Mi"}}}],
                     "restartPolicy": "Always"}}}
    });
    time("emit_yaml_from_value(manifest)", 20000, || {
        emit_yaml_from_value(&doc)
    });

    // 5. encode/decode path round trip
    let p = "spec.template.spec.containers[0].resources.requests";
    time("encode_path + decode_path", 200000, || {
        decode_path(&encode_path(p)).unwrap()
    });

    // 6. set_nested hot path (form edit simulation)
    time("set_nested(deep path)", 200000, || {
        let mut v = serde_json::json!({});
        set_nested(
            &mut v,
            &["spec", "template", "spec", "containers[0]", "image"],
            serde_json::json!("nginx"),
        );
        v
    });

    // 7. Full cold-init cost (index + ALL shards parsed)
    time("ALL shards parse (all 22 kinds)", 1, || {
        let mut n = 0usize;
        for r in get_all_resources_ref() {
            n += load_resource(&r.kind)
                .map(|v| v.as_object().map_or(0, |m| m.len()))
                .unwrap_or(0);
        }
        n
    });

    println!("\nleaf path counts:");
    for kind in ["Pod", "Deployment", "CronJob"] {
        let f = load_resource(kind).unwrap();
        let n = get_leaf_field_paths(&f["fields"]).len();
        println!("  {kind:<14} {n:>6} leaf fields");
    }
}
