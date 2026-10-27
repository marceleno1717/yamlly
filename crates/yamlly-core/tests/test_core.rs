use serde_json::json;
use yamlly_core::*;

#[test]
fn test_set_nested_dict() {
    let mut data = json!({});
    set_nested(&mut data, &["a", "b", "c"], json!(42));
    assert_eq!(data["a"]["b"]["c"], json!(42));
}

#[test]
fn test_set_nested_list() {
    let mut data = json!({});
    set_nested(&mut data, &["a", "b[0]", "c"], json!(42));
    assert_eq!(data["a"]["b"][0]["c"], json!(42));
}

#[test]
fn test_set_nested_list_append() {
    let mut data = json!({"a": {"b": [{"c": 1}]}});
    set_nested(&mut data, &["a", "b[1]", "c"], json!(2));
    assert_eq!(data["a"]["b"][1]["c"], json!(2));
}

#[test]
fn test_get_nested() {
    let data = json!({"a": {"b": [{"c": 42}]}});
    assert_eq!(get_nested(&data, &["a", "b[0]", "c"]), Some(&json!(42)));
    assert_eq!(get_nested(&data, &["a", "b", "c"]), None);
}

#[test]
fn test_encode_decode_path() {
    let path = "spec.template.spec.containers[0].resources.requests";
    let encoded = encode_path(path);
    assert!(encoded.starts_with("sf_"));
    assert!(encoded.contains("__"));

    let decoded = decode_path(&encoded).expect("should decode");
    assert_eq!(
        decoded,
        vec![
            "spec",
            "template",
            "spec",
            "containers[0]",
            "resources",
            "requests"
        ]
    );
}

#[test]
fn test_decode_invalid() {
    assert_eq!(decode_path("invalid_id"), None);
}

#[test]
fn test_get_all_resources() {
    let res = get_all_resources();
    assert!(res.len() >= 22);
    assert!(res.iter().any(|r| r.kind == "Pod"));
}

#[test]
fn test_load_resource() {
    let pod = load_resource("Pod").expect("Pod should exist");
    assert_eq!(pod["kind"], "Pod");
    assert_eq!(pod["apiVersion"], "v1");
    assert!(pod.get("fields").is_some());
}

#[test]
fn test_get_common_fields() {
    let fields = get_common_fields("Pod");
    assert!(!fields.is_empty());
    assert!(
        fields.contains(&"spec.containers".to_string())
            || fields.contains(&"metadata.name".to_string())
    );
}

#[test]
fn test_get_leaf_field_paths() {
    let pod = load_resource("Pod").expect("Pod schema should load");
    let fields = pod.get("fields").expect("fields exist");
    let paths = get_leaf_field_paths(fields);
    assert!(paths.len() > 100);
}

#[test]
fn test_leaf_field_cache_matches_uncached() {
    // Cached variant must be byte-identical to the uncached walk, including
    // the exact-match (non-prefix) skip list, for every shard.
    for entry in get_all_resources_ref() {
        let uncached =
            get_leaf_field_paths(&load_resource(&entry.kind).expect("shard loads")["fields"]);
        let cached = get_leaf_field_paths_cached(&entry.kind)
            .unwrap_or_else(|| panic!("cache miss for {}", entry.kind));
        assert_eq!(uncached.len(), cached.len(), "{}", entry.kind);
        for (a, b) in uncached.iter().zip(cached.iter()) {
            assert_eq!(a.path, b.path, "{}", entry.kind);
            assert_eq!(a.field_type, b.field_type, "{}", entry.kind);
            assert_eq!(a.required, b.required, "{}", entry.kind);
        }
        // Repeated hits must be stable.
        let again = get_leaf_field_paths_cached(&entry.kind).unwrap();
        assert_eq!(again.len(), cached.len());
    }
}

#[test]
fn test_leaf_field_cache_rejects_unknown_kind() {
    assert!(get_leaf_field_paths_cached("NotARealKind").is_none());
}

#[test]
fn test_emit_owned_matches_borrowed() {
    let data = json!({
        "kind": "Pod", "apiVersion": "v1",
        "metadata": {"name": "x", "labels": {}},
        "spec": {"containers": [], "replicas": 2}
    });
    assert_eq!(emit_yaml_owned(data.clone()), emit_yaml_from_value(&data));
}

#[test]
fn test_clean_removes_empty() {
    let data = json!({
        "metadata": {"name": "test", "labels": {}},
        "spec": {"containers": [], "replicas": 1},
        "empty_str": ""
    });
    let cleaned = clean(&data);
    assert_eq!(
        cleaned,
        json!({
            "metadata": {"name": "test"},
            "spec": {"replicas": 1}
        })
    );
}

#[test]
fn test_emit_yaml_basic() {
    let data = json!({"kind": "Pod", "apiVersion": "v1", "metadata": {"name": "x"}});
    let yaml_str = emit_yaml_from_value(&data);
    assert!(yaml_str.contains("kind: Pod"));
    assert!(yaml_str.contains("apiVersion: v1"));
    assert!(yaml_str.contains("  name: x"));
}
