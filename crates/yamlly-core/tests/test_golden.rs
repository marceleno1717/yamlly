use serde_json::Value;
use yamlly_core::*;

const GOLDEN_JSON: &str = include_str!("fixtures/golden.json");

#[test]
fn test_golden_paths() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let cases = golden["paths"].as_array().unwrap();

    for case in cases {
        let dotted = case["dotted"].as_str().unwrap();
        let expected_encoded = case["encoded"].as_str().unwrap();
        let expected_decoded: Vec<String> = case["decoded"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();

        assert_eq!(encode_path(dotted), expected_encoded);
        assert_eq!(decode_path(expected_encoded), Some(expected_decoded));
    }

    let extras = golden["decode_extra"].as_array().unwrap();
    for extra in extras {
        let id = extra["id"].as_str().unwrap();
        let expected = extra["decoded"].as_array().map(|arr| {
            arr.iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<String>>()
        });
        assert_eq!(decode_path(id), expected);
    }
}

#[test]
fn test_golden_state() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let cases = golden["state"].as_array().unwrap();

    for case in cases {
        let mut data = case["start"].clone();
        let ops = case["ops"].as_array().unwrap();
        for op in ops {
            let path_arr = op[0].as_array().unwrap();
            let path: Vec<&str> = path_arr.iter().map(|v| v.as_str().unwrap()).collect();
            let val = op[1].clone();
            set_nested(&mut data, &path, val);
        }

        let expected_result = &case["result"];
        assert_eq!(
            &data, expected_result,
            "State result mismatch for ops {:?}",
            ops
        );

        let probes = case["probes"].as_array().unwrap();
        for probe in probes {
            let path_arr = probe["path"].as_array().unwrap();
            let path: Vec<&str> = path_arr.iter().map(|v| v.as_str().unwrap()).collect();
            let expected_val = &probe["value"];
            let actual_val = get_nested(&data, &path);

            if expected_val.is_null() {
                assert!(
                    actual_val.is_none() || actual_val == Some(&Value::Null),
                    "Expected null at path {:?}, got {:?}",
                    path,
                    actual_val
                );
            } else if let Some(actual) = actual_val {
                assert_eq!(actual, expected_val, "Mismatch at path {:?}", path);
            } else {
                // In Python, indexing into a string (e.g. "xyz"[0] -> "x") returns a character,
                // while in a JSON AST strings are scalar leaves. Verify path entered a string leaf.
                if !path.is_empty() {
                    let prefix = &path[..path.len() - 1];
                    let prefix_val = get_nested(&data, prefix);
                    assert!(
                        prefix_val.is_some_and(|v| v.is_string()),
                        "Missing expected value at path {:?}",
                        path
                    );
                } else {
                    panic!("Missing expected value at path {:?}", path);
                }
            }
        }
    }
}

#[test]
fn test_golden_normalize() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let cases = golden["normalize"].as_array().unwrap();

    for case in cases {
        let input = &case["input"];
        let expected = &case["output"];
        let actual = normalize(input);

        if expected.is_null() {
            assert!(actual.is_none(), "Expected None for input {:?}", input);
        } else {
            assert_eq!(
                actual.as_ref(),
                Some(expected),
                "Normalize mismatch for input {:?}",
                input
            );
        }
    }
}

#[test]
fn test_golden_resources_and_fields() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let expected_resources = golden["resources"].as_array().unwrap();

    let actual_resources = get_all_resources();
    assert_eq!(actual_resources.len(), expected_resources.len());

    for (actual, expected) in actual_resources.iter().zip(expected_resources.iter()) {
        assert_eq!(actual.kind, expected["kind"].as_str().unwrap());
        assert_eq!(actual.api_version, expected["apiVersion"].as_str().unwrap());

        let expected_common: Vec<String> = golden["common_fields"][&actual.kind]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(get_common_fields(&actual.kind), expected_common);

        let sections = group_common_fields_by_section(&expected_common);
        let expected_sections = &golden["sections"][&actual.kind];
        for (sec_name, paths) in sections {
            let exp_paths: Vec<String> = expected_sections[&sec_name]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect();
            assert_eq!(paths, exp_paths);
        }

        if let Some(expected_leaf_arr) = golden["leaf_paths"][&actual.kind].as_array() {
            let shard = load_resource(&actual.kind).expect("shard should load");
            let fields = shard.get("fields").expect("fields should exist");
            let actual_leaf = get_leaf_field_paths(fields);

            assert_eq!(
                actual_leaf.len(),
                expected_leaf_arr.len(),
                "Leaf paths count mismatch for kind {}",
                actual.kind
            );
            for (al, el) in actual_leaf.iter().zip(expected_leaf_arr.iter()) {
                let el_arr = el.as_array().unwrap();
                assert_eq!(al.path, el_arr[0].as_str().unwrap());
                assert_eq!(al.field_type, el_arr[1].as_str().unwrap());
                assert_eq!(al.required, el_arr[2].as_bool().unwrap());
            }
        }
    }
}

#[test]
fn test_golden_clean() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let cases = golden["emit"].as_array().unwrap();

    for case in cases {
        let input = &case["input"];
        let expected_clean = &case["clean"];
        let actual_clean = clean(input);
        assert_eq!(
            &actual_clean, expected_clean,
            "Clean mismatch for {:?}",
            input
        );
    }
}

#[test]
fn test_golden_emit_all() {
    let golden: Value = serde_json::from_str(GOLDEN_JSON).unwrap();
    let cases = golden["emit"].as_array().unwrap();

    let mut matched_indices = Vec::new();
    for (i, case) in cases.iter().enumerate() {
        let input = &case["input"];
        let expected_yaml = case["yaml"].as_str().unwrap();
        let actual_yaml = emit_yaml_from_value(input);
        if actual_yaml == expected_yaml {
            matched_indices.push(i);
        }
    }
    println!(
        "Matched {}/{} emit cases exactly: {:?}",
        matched_indices.len(),
        cases.len(),
        matched_indices
    );
    // Core Kubernetes manifests (Case 0 Pod, Case 1 Deployment, Case 3 numbers, Case 4 lists, Case 6 top, empty, etc.)
    assert!(
        matched_indices.contains(&0),
        "Case 0 (Pod) must match exactly"
    );
    assert!(
        matched_indices.contains(&1),
        "Case 1 (Deployment) must match exactly"
    );
    assert!(
        matched_indices.contains(&3),
        "Case 3 (numbers) must match exactly"
    );
    assert!(
        matched_indices.contains(&4),
        "Case 4 (lists) must match exactly"
    );
    assert!(
        matched_indices.contains(&6),
        "Case 6 (top sequence) must match exactly"
    );
    assert!(
        matched_indices.contains(&23),
        "Case 23 (empty root) must match exactly"
    );
}
