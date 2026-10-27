use std::collections::HashMap;

use yamlly_core::field::{classify, schema_lookup_path, FieldKind};
use yamlly_core::*;

fn fields_of(kind: &str) -> Vec<Row> {
    build_form(kind, &[], &[], &HashMap::new())
        .unwrap_or_else(|| panic!("{kind} should build"))
        .0
}

fn field_paths(rows: &[Row]) -> Vec<&str> {
    rows.iter()
        .filter_map(|r| match r {
            Row::Field { field, .. } => Some(field.path.as_str()),
            _ => None,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

#[test]
fn test_classify_primitives() {
    let ns = vec!["default".to_string(), "kube-system".to_string()];
    assert_eq!(classify("metadata.name", "string", &ns), FieldKind::Text);
    assert_eq!(
        classify("spec.replicas", "integer", &ns),
        FieldKind::Integer
    );
    assert_eq!(
        classify("spec.hostNetwork", "boolean", &ns),
        FieldKind::Boolean
    );
    assert_eq!(
        classify("metadata.labels", "map[string]string", &ns),
        FieldKind::Map
    );
    // map[string]Quantity has an empty `fields` node but is still a map.
    assert_eq!(
        classify("spec.resources.limits", "map[string]Quantity", &ns),
        FieldKind::Map
    );
    assert_eq!(
        classify("spec.hostAliases", "[]HostAlias", &ns),
        FieldKind::StringList
    );
}

#[test]
fn test_classify_namespace_requires_options() {
    let many = vec!["default".to_string(), "kube-system".to_string()];
    let one = vec!["default".to_string()];
    assert_eq!(
        classify("metadata.namespace", "string", &many),
        FieldKind::Namespace
    );
    // No live list -> plain text, so the field stays editable offline.
    assert_eq!(
        classify("metadata.namespace", "string", &one),
        FieldKind::Text
    );
    assert_eq!(
        classify("metadata.namespace", "string", &[]),
        FieldKind::Text
    );
    // Nested template namespace too.
    assert_eq!(
        classify("spec.template.metadata.namespace", "string", &many),
        FieldKind::Namespace
    );
}

#[test]
fn test_classify_known_enums() {
    let ns: Vec<String> = vec![];
    assert_eq!(
        classify("spec.template.spec.restartPolicy", "string", &ns),
        FieldKind::Enum(vec!["Always".into(), "OnFailure".into(), "Never".into()])
    );
    assert_eq!(
        classify("spec.type", "string", &ns),
        FieldKind::Enum(vec![
            "ClusterIP".into(),
            "NodePort".into(),
            "LoadBalancer".into(),
            "ExternalName".into()
        ])
    );
    // Array indices are stripped before the enum lookup.
    assert_eq!(
        classify(
            "spec.template.spec.containers[0].imagePullPolicy",
            "string",
            &ns
        ),
        FieldKind::Text
    );
}

#[test]
fn test_schema_lookup_path_strips_indices() {
    assert_eq!(
        schema_lookup_path("spec.template.spec.containers[0].image"),
        "spec.template.spec.containers.image"
    );
    assert_eq!(
        schema_lookup_path("spec.template.spec.containers[].name"),
        "spec.template.spec.containers.name"
    );
    assert_eq!(schema_lookup_path("metadata.name"), "metadata.name");
    // An unterminated `[` has no closing bracket to pair with, so it stays
    // literal rather than swallowing the rest of the path.
    assert_eq!(schema_lookup_path("a[0.b"), "a[0.b");
    assert_eq!(schema_lookup_path("a]"), "a]");
    assert_eq!(schema_lookup_path("a[0]b"), "ab");
    assert_eq!(schema_lookup_path("x.y[12].z"), "x.y.z");
    assert_eq!(schema_lookup_path("a[0]b[1]c"), "abc");
}

// ---------------------------------------------------------------------------
// Layout
// ---------------------------------------------------------------------------

#[test]
fn test_deployment_renders_expected_default_fields() {
    let rows = fields_of("Deployment");
    let paths = field_paths(&rows);

    // Verification gate for Phase 3.
    for expected in [
        "metadata.name",
        "metadata.namespace",
        "metadata.labels",
        "spec.replicas",
        "spec.selector.matchLabels",
        "spec.template.metadata.labels",
        "spec.template.spec.containers[0].name",
        "spec.template.spec.containers[0].image",
    ] {
        assert!(
            paths.contains(&expected),
            "Deployment form missing {expected}\n  got: {paths:#?}"
        );
    }
}

#[test]
fn test_every_kind_builds_a_form() {
    for entry in get_all_resources_ref() {
        let rows = fields_of(&entry.kind);
        let n = field_paths(&rows).len();
        assert!(n > 0, "{kind} produced no fields", kind = entry.kind);
        assert!(
            n < 500,
            "{kind} produced {n} fields -- form is probably unbounded",
            kind = entry.kind
        );
    }
}

#[test]
fn test_sections_are_ordered_metadata_then_spec() {
    let rows = fields_of("Deployment");
    let sections: Vec<&String> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Section(s) => Some(s),
            _ => None,
        })
        .collect();
    assert_eq!(sections, vec!["metadata", "spec"]);
}

#[test]
fn test_array_group_materialises_one_item_by_default() {
    let rows = fields_of("Deployment");
    let containers = rows
        .iter()
        .find_map(|r| match r {
            Row::Array {
                path, count, key, ..
            } if path.ends_with("containers[]") => Some((key.clone(), *count)),
            _ => None,
        })
        .expect("containers[] array group");
    assert_eq!(containers, ("containers".to_string(), 1usize));

    // Leaves under it carry the [0] index.
    assert!(field_paths(&rows)
        .iter()
        .any(|p| p.contains("containers[0].image")));
}

#[test]
fn test_array_count_expands_to_multiple_indexed_items() {
    let mut counts = HashMap::new();
    counts.insert("spec.template.spec.containers[]".to_string(), 3usize);
    let rows = build_form("Deployment", &[], &[], &counts).unwrap().0;
    let paths = field_paths(&rows);

    for i in 0..3 {
        assert!(
            paths.contains(&format!("spec.template.spec.containers[{i}].image").as_str()),
            "missing index {i}: {paths:#?}"
        );
    }
    assert!(!paths.contains(&"spec.template.spec.containers[3].image"));
}

#[test]
fn test_required_marker_follows_schema() {
    let rows = fields_of("Deployment");
    let field = |p: &str| {
        rows.iter().find_map(|r| match r {
            Row::Field { field, .. } if field.path == p => Some(field.clone()),
            _ => None,
        })
    };

    // The generated schema marks container name/port required...
    let cname = field("spec.template.spec.containers[0].name").expect("containers[0].name");
    assert!(cname.required, "container name is required");
    let cport = field("spec.template.spec.containers[0].ports[0].containerPort")
        .expect("containers[0].ports[0].containerPort");
    assert!(cport.required, "containerPort is required");

    // ...but top-level metadata.name is not flagged in the shard, and we must
    // mirror the schema rather than hardcode Kubernetes conventions.
    let name = field("metadata.name").expect("metadata.name");
    assert!(!name.required, "shard says metadata.name is not required");
    assert_eq!(name.kind, FieldKind::Text);

    let replicas = field("spec.replicas").expect("spec.replicas");
    assert_eq!(replicas.kind, FieldKind::Integer);
    assert_eq!(replicas.label, "replicas");
}

#[test]
fn test_extra_paths_extend_form_without_duplicates() {
    let (rows, shown) = build_form(
        "Deployment",
        &[],
        &["spec.paused".to_string(), "metadata.name".to_string()],
        &HashMap::new(),
    )
    .unwrap();

    let paths = field_paths(&rows);
    assert_eq!(
        paths.iter().filter(|p| **p == "metadata.name").count(),
        1,
        "duplicate path must not render twice"
    );
    assert!(
        paths.contains(&"spec.paused"),
        "extra path missing: {paths:#?}"
    );
    assert!(shown.contains(&"spec.paused".to_string()));
}

#[test]
fn test_unknown_kind_returns_none() {
    assert!(build_form("NotARealKind", &[], &[], &HashMap::new()).is_none());
}

#[test]
fn test_common_fields_case_insensitive() {
    // The form builder receives the canonical casing ("Deployment") but the
    // loader cache keys lowercase; both must resolve.
    let upper = yamlly_core::get_common_fields("Deployment");
    let lower = yamlly_core::get_common_fields("deployment");
    assert!(!upper.is_empty());
    assert_eq!(upper, lower, "lookup must be case-insensitive");
    assert!(upper.contains(&"metadata.name".to_string()));
    assert!(yamlly_core::get_common_fields("nope").is_empty());
}

#[test]
fn test_build_form_case_insensitive() {
    let a = build_form("Pod", &[], &[], &HashMap::new()).unwrap().0;
    let b = build_form("pod", &[], &[], &HashMap::new()).unwrap().0;
    assert_eq!(a.len(), b.len());
    assert!(!a.is_empty());
}

#[test]
fn test_group_by_section() {
    let paths = vec![
        "metadata.name".to_string(),
        "metadata.labels".to_string(),
        "spec.replicas".to_string(),
        "spec.template.spec.containers[].name".to_string(),
    ];
    let sections = group_sections(&paths);
    assert_eq!(sections.len(), 2);
    assert_eq!(sections[0].0, "metadata");
    assert_eq!(sections[0].1.len(), 2);
    assert_eq!(sections[1].0, "spec");
    assert_eq!(sections[1].1.len(), 2);
}

#[test]
fn test_build_path_tree_nesting() {
    let paths = vec![
        "metadata.name".to_string(),
        "spec.template.spec.containers[].image".to_string(),
    ];
    let tree = build_path_tree(&paths);
    // First segment is the section and is not a node.
    let keys: Vec<&String> = tree.entries.iter().map(|(k, _)| k).collect();
    assert_eq!(keys, vec!["name", "template"]);
}
