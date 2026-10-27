use serde_json::json;
use yamlly::app::App;
use yamlly::form::{FormState, Mode, split_data_path};
use yamlly_core::field::Row;

/// Focus the field at `path`, failing the test if it is not present.
fn focus(form: &mut FormState, path: &str) {
    let idx = form
        .rows
        .iter()
        .position(|r| matches!(&r.kind, Row::Field { field, .. } if field.path == path))
        .unwrap_or_else(|| panic!("field {path} not found in {:?}", paths(form)));
    form.focus = idx;
    form.mode = Mode::Navigate;
}

fn paths(form: &FormState) -> Vec<&str> {
    form.rows
        .iter()
        .filter_map(|r| match &r.kind {
            Row::Field { field, .. } => Some(field.path.as_str()),
            _ => None,
        })
        .collect()
}

fn value(form: &FormState, path: &str) -> Option<serde_json::Value> {
    form.field_value(path).cloned()
}

// ---------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------

#[test]
fn test_form_seeds_kind_and_api_version() {
    let f = FormState::new("Deployment", None, vec![]);
    assert_eq!(f.data["kind"], json!("Deployment"));
    assert_eq!(f.data["apiVersion"], json!("apps/v1"));
    assert!(!f.is_dirty(), "a freshly opened form must not be dirty");
}

#[test]
fn test_form_renders_deployment_fields() {
    // Verification gate: Deployment renders its default fields.
    let f = FormState::new("Deployment", None, vec![]);
    let p = paths(&f);
    for expected in [
        "metadata.name",
        "metadata.namespace",
        "spec.replicas",
        "spec.template.spec.containers[0].image",
    ] {
        assert!(p.contains(&expected), "missing {expected}: {p:?}");
    }
}

#[test]
fn test_form_seeded_from_existing_document_is_clean() {
    let doc = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {"name": "web"},
        "spec": {"replicas": 3}
    });
    let f = FormState::new("Deployment", Some(&doc), vec![]);
    assert!(!f.is_dirty(), "opened file should match its own baseline");
    assert_eq!(value(&f, "metadata.name"), Some(json!("web")));
    assert_eq!(value(&f, "spec.replicas"), Some(json!(3)));
}

#[test]
fn test_unknown_kind_yields_empty_form_without_panic() {
    let f = FormState::new("NotARealKind", None, vec![]);
    assert!(f.rows.is_empty());
    assert!(!f.is_dirty());
}

// ---------------------------------------------------------------------------
// Path handling
// ---------------------------------------------------------------------------

#[test]
fn test_split_data_path() {
    assert_eq!(split_data_path("metadata.name"), vec!["metadata", "name"]);
    assert_eq!(
        split_data_path("spec.template.spec.containers[0].image"),
        vec!["spec", "template", "spec", "containers", "0", "image"]
    );
    assert_eq!(
        split_data_path("spec.containers[0].env[2].name"),
        vec!["spec", "containers", "0", "env", "2", "name"]
    );
}

// ---------------------------------------------------------------------------
// Editing
// ---------------------------------------------------------------------------

#[test]
fn test_typing_updates_document() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    f.begin_edit();
    assert_eq!(f.mode, Mode::Edit);
    for c in "web".chars() {
        f.insert_char(c);
    }
    f.commit();
    assert_eq!(value(&f, "metadata.name"), Some(json!("web")));
}

#[test]
fn test_backspace_edits_buffer() {
    let mut f = FormState::new(
        "Deployment",
        Some(&json!({"metadata":{"name":"web"}})),
        vec![],
    );
    focus(&mut f, "metadata.name");
    f.begin_edit();
    assert_eq!(f.buffer, "web");
    f.delete_char();
    assert_eq!(f.buffer, "we");
    f.commit();
    assert_eq!(value(&f, "metadata.name"), Some(json!("we")));
}

#[test]
fn test_escape_discards_edit() {
    let mut f = FormState::new(
        "Deployment",
        Some(&json!({"metadata":{"name":"web"}})),
        vec![],
    );
    focus(&mut f, "metadata.name");
    f.begin_edit();
    f.insert_char('X');
    assert_eq!(f.buffer, "webX");
    f.cancel();
    assert_eq!(f.mode, Mode::Navigate);
    // Document untouched by the abandoned edit.
    assert_eq!(value(&f, "metadata.name"), Some(json!("web")));
}

#[test]
fn test_integer_field_coerces_to_number() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "spec.replicas");
    f.begin_edit();
    for c in "3".chars() {
        f.insert_char(c);
    }
    f.commit();
    assert_eq!(value(&f, "spec.replicas"), Some(json!(3)));
    assert!(
        value(&f, "spec.replicas").unwrap().is_number(),
        "integer field must emit a JSON number, not a string"
    );
}

#[test]
fn test_non_numeric_integer_kept_as_text_for_validation() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "spec.replicas");
    f.begin_edit();
    for c in "abc".chars() {
        f.insert_char(c);
    }
    f.commit();
    assert_eq!(value(&f, "spec.replicas"), Some(json!("abc")));
}

#[test]
fn test_empty_value_removes_key() {
    let mut f = FormState::new("Deployment", Some(&json!({"spec":{"replicas":2}})), vec![]);
    focus(&mut f, "spec.replicas");
    f.begin_edit();
    for _ in 0..4 {
        f.delete_char();
    }
    f.commit();
    // Empty collapses to nothing rather than persisting an empty string.
    assert_eq!(value(&f, "spec.replicas"), None);
}

// ---------------------------------------------------------------------------
// Toggles and dropdowns
// ---------------------------------------------------------------------------

#[test]
fn test_boolean_toggles_immediately() {
    // Pod's default fields carry no boolean, so widen the form with the path
    // the schema marks as one. This exercises the same code path without
    // depending on which kinds happen to surface booleans by default.
    let mut f = FormState::new("Pod", None, vec![]);
    assert!(
        !paths(&f).contains(&"spec.hostNetwork"),
        "precondition: hostNetwork is not a default field"
    );
    f.shown_paths.push("spec.hostNetwork".into());
    f.rebuild_with_extra(&["spec.hostNetwork".into()]);
    focus(&mut f, "spec.hostNetwork");
    // An unset switch renders as "false", so the first Enter turns it on.
    assert!(
        value(&f, "spec.hostNetwork").is_none(),
        "boolean starts unset"
    );
    f.begin_edit();
    assert_eq!(f.mode, Mode::Navigate, "boolean must not enter edit mode");
    assert_eq!(value(&f, "spec.hostNetwork"), Some(json!(true)));

    // Subsequent toggles flip, and stay in Navigate mode (never edit mode).
    f.begin_edit();
    assert_eq!(f.mode, Mode::Navigate);
    assert_eq!(value(&f, "spec.hostNetwork"), Some(json!(false)));
    f.begin_edit();
    assert_eq!(value(&f, "spec.hostNetwork"), Some(json!(true)));
}

#[test]
fn test_enum_cycles_options() {
    let mut f = FormState::new("Pod", None, vec![]);
    focus(&mut f, "spec.restartPolicy");
    f.begin_edit();
    assert_eq!(f.mode, Mode::Select);
    assert_eq!(value(&f, "spec.restartPolicy"), Some(json!("Always")));
    f.move_cursor(true);
    assert_eq!(value(&f, "spec.restartPolicy"), Some(json!("OnFailure")));
    f.move_cursor(true);
    assert_eq!(value(&f, "spec.restartPolicy"), Some(json!("Never")));
    f.move_cursor(true);
    assert_eq!(value(&f, "spec.restartPolicy"), Some(json!("Always")));
}

#[test]
fn test_namespace_field_uses_live_list() {
    let ns = vec!["default".to_string(), "kube-system".to_string()];
    let mut f = FormState::new("Pod", None, ns);
    focus(&mut f, "metadata.namespace");
    f.begin_edit();
    assert_eq!(f.mode, Mode::Select);
    assert_eq!(value(&f, "metadata.namespace"), Some(json!("default")));
    f.move_cursor(true);
    assert_eq!(value(&f, "metadata.namespace"), Some(json!("kube-system")));
}

// ---------------------------------------------------------------------------
// Arrays
// ---------------------------------------------------------------------------

#[test]
fn test_add_array_item_materialises_new_index() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "spec.template.spec.containers[0].name");
    assert!(f.add_array_item());

    let p = paths(&f);
    assert!(
        p.contains(&"spec.template.spec.containers[1].name"),
        "second container not materialised: {p:?}"
    );

    // The new item is independently editable and writes to its own index.
    focus(&mut f, "spec.template.spec.containers[1].name");
    f.begin_edit();
    for c in "api".chars() {
        f.insert_char(c);
    }
    f.commit();
    assert_eq!(
        value(&f, "spec.template.spec.containers[1].name"),
        Some(json!("api"))
    );
    // ...without disturbing the first.
    assert_eq!(value(&f, "spec.template.spec.containers[0].name"), None);
}

#[test]
fn test_add_array_item_on_scalar_is_rejected() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    assert!(!f.add_array_item(), "scalar fields have no array to grow");
}

// ---------------------------------------------------------------------------
// Navigation
// ---------------------------------------------------------------------------

#[test]
fn test_focus_navigation_skips_headers() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    f.focus_next_field(true);
    assert_eq!(f.focused_field().expect("field").path, "metadata.namespace");

    // From here, walk every field exactly once and land back on the start.
    let start = f.focused_field().unwrap().path.clone();
    let total = paths(&f).len();
    let mut seen = Vec::new();
    for _ in 0..total {
        f.focus_next_field(true);
        seen.push(f.focused_field().unwrap().path.clone());
    }
    assert_eq!(seen.len(), total);
    assert_eq!(
        seen.last().unwrap(),
        &start,
        "navigation should wrap back to the starting field"
    );
    // Every field was reachable.
    assert_eq!(
        seen.iter().collect::<std::collections::HashSet<_>>().len(),
        total
    );
}

#[test]
fn test_focus_navigation_backwards_wraps() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    f.focus_next_field(false);
    let last = f.focused_field().unwrap().path.clone();
    f.focus_next_field(false);
    // Moving back from the first field lands on the last field.
    let second_last = f.focused_field().unwrap().path.clone();
    assert_ne!(last, second_last);
}

#[test]
fn test_ensure_visible_scrolls_to_focus() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    f.ensure_visible(10);
    assert_eq!(f.scroll(), 0);

    // Jump near the end; the viewport must follow.
    let last = f.rows.len() - 1;
    f.focus = last;
    f.ensure_visible(10);
    assert!(f.scroll() + 10 > last, "focused row scrolled out of view");
}

// ---------------------------------------------------------------------------
// Dirty tracking
// ---------------------------------------------------------------------------

#[test]
fn test_dirty_flips_on_edit_and_back() {
    let mut f = FormState::new("Deployment", None, vec![]);
    assert!(!f.is_dirty());

    focus(&mut f, "metadata.name");
    f.begin_edit();
    for c in "web".chars() {
        f.insert_char(c);
    }
    f.commit();
    assert!(f.is_dirty(), "editing must mark the form dirty");

    f.mark_clean();
    assert!(!f.is_dirty(), "mark_clean must clear the dirty flag");
}

#[test]
fn test_reverting_edit_returns_to_clean() {
    let mut f = FormState::new(
        "Deployment",
        Some(&json!({"metadata":{"name":"web"}})),
        vec![],
    );
    focus(&mut f, "metadata.name");
    f.begin_edit();
    f.insert_char('X');
    f.commit();
    assert!(f.is_dirty());

    // Undo the edit: back to the original value.
    f.begin_edit();
    f.delete_char();
    f.commit();
    assert!(
        !f.is_dirty(),
        "reverting to the loaded value must clear dirty"
    );
}

#[test]
fn test_yaml_emits_edited_values() {
    let mut f = FormState::new("Deployment", None, vec![]);
    focus(&mut f, "metadata.name");
    f.begin_edit();
    for c in "web".chars() {
        f.insert_char(c);
    }
    f.commit();
    focus(&mut f, "spec.replicas");
    f.begin_edit();
    for c in "3".chars() {
        f.insert_char(c);
    }
    f.commit();

    let yaml = yamlly_core::emit_yaml_owned(f.data.clone());
    assert!(yaml.contains("name: web"), "{yaml}");
    assert!(yaml.contains("replicas: 3"), "{yaml}");
}

// ---------------------------------------------------------------------------
// App integration
// ---------------------------------------------------------------------------

#[test]
fn test_app_open_resource_creates_editable_form() {
    let mut app = App::new_test();
    assert!(app.active_form_mut().is_none());

    app.open_selected_resource();
    let form = app.active_form_mut().expect("form after opening a tab");
    assert_eq!(form.kind, "ClusterRole");
    assert!(!app.tabs[0].dirty);

    // Editing through the form marks the tab dirty after refresh.
    {
        let form = app.active_form_mut().unwrap();
        let idx = form
            .rows
            .iter()
            .position(|r| matches!(&r.kind, Row::Field { .. }))
            .expect("a field");
        form.focus = idx;
        form.begin_edit();
        for c in "abc".chars() {
            form.insert_char(c);
        }
        form.commit();
    }
    app.refresh_tab_state();
    assert!(app.tabs[0].dirty, "tab must show dirty after an edit");
}

#[test]
fn test_app_tab_label_follows_name_field() {
    let mut app = App::new_test();
    app.open_selected_resource();
    let original = app.tabs[0].label.clone();

    // ClusterRole's first field is metadata.name.
    {
        let form = app.active_form_mut().unwrap();
        let idx = form
            .rows
            .iter()
            .position(
                |r| matches!(&r.kind, Row::Field { field, .. } if field.path == "metadata.name"),
            )
            .expect("metadata.name field");
        form.focus = idx;
        form.begin_edit();
        for c in "my-role".chars() {
            form.insert_char(c);
        }
        form.commit();
    }
    app.refresh_tab_state();
    assert_eq!(app.tabs[0].label, "ClusterRole: my-role");
    assert_ne!(app.tabs[0].label, original);
}

#[test]
fn test_app_tabs_are_isolated() {
    let mut app = App::new_test();
    app.open_selected_resource(); // ClusterRole
    app.select_next_resource();
    app.open_selected_resource(); // ClusterRoleBinding

    {
        let form = app.active_form_mut().unwrap();
        let idx = form
            .rows
            .iter()
            .position(|r| matches!(&r.kind, Row::Field { .. }))
            .unwrap();
        form.focus = idx;
        form.begin_edit();
        form.insert_char('x');
        form.commit();
    }

    // Switch back: the first tab must not see the second tab's value.
    app.cycle_tab(false);
    assert_eq!(app.active_tab_idx, Some(0));
    let form = app.active_form_mut().unwrap();
    assert!(
        form.data["metadata"]["name"].as_str() != Some("x"),
        "editing bled across tabs"
    );
}

#[test]
fn test_app_active_yaml_reflects_form() {
    let mut app = App::new_test();
    assert!(app.active_yaml().is_none());
    app.open_selected_resource();
    let yaml = app.active_yaml().expect("yaml once a tab is open");
    assert!(yaml.contains("kind: ClusterRole"), "{yaml}");
}

#[test]
fn test_app_cycle_tab_wraps() {
    let mut app = App::new_test();
    app.open_selected_resource();
    app.select_next_resource();
    app.open_selected_resource();
    assert_eq!(app.active_tab_idx, Some(1));
    app.cycle_tab(true);
    assert_eq!(app.active_tab_idx, Some(0));
    app.cycle_tab(false);
    assert_eq!(app.active_tab_idx, Some(1), "backward cycle must wrap");
}

#[test]
fn test_added_field_placed_inside_its_category() {
    let mut f = FormState::new("Deployment", None, vec![]);
    f.rebuild_with_extra(&[
        "spec.minReadySeconds".to_string(),
        "spec.template.spec.restartPolicy".to_string(),
        "spec.template.spec.containers[].workingDir".to_string(),
    ]);

    // 1. spec.minReadySeconds must be placed directly inside spec (after spec.replicas, before selector)
    let rows: Vec<String> = f
        .rows
        .iter()
        .map(|r| match &r.kind {
            Row::Section(s) => format!("sec:{s}"),
            Row::Group { label, depth } => format!("grp:{depth}:{label}"),
            Row::Array { key, depth, .. } => format!("arr:{depth}:{key}"),
            Row::Field { field, depth } => format!("fld:{depth}:{}", field.path),
        })
        .collect();

    let idx_replicas = rows
        .iter()
        .position(|r| r == "fld:1:spec.replicas")
        .expect("replicas");
    let idx_min_ready = rows
        .iter()
        .position(|r| r == "fld:1:spec.minReadySeconds")
        .expect("minReadySeconds");
    let idx_selector = rows
        .iter()
        .position(|r| r == "grp:1:selector")
        .expect("selector");
    let idx_template = rows
        .iter()
        .position(|r| r == "grp:1:template")
        .expect("template");

    assert!(
        idx_min_ready > idx_replicas,
        "minReadySeconds must follow replicas"
    );
    assert!(
        idx_min_ready < idx_selector,
        "minReadySeconds must be placed at top of spec before selector"
    );
    assert!(
        idx_min_ready < idx_template,
        "minReadySeconds must be placed at top of spec before template"
    );

    // 2. spec.template.spec.restartPolicy must be placed directly inside spec.template.spec before containers[]
    let idx_restart_policy = rows
        .iter()
        .position(|r| r.ends_with(":spec.template.spec.restartPolicy"))
        .expect("restartPolicy");
    let idx_containers = rows
        .iter()
        .position(|r| r.starts_with("arr:") && r.ends_with(":containers"))
        .expect("containers array");

    assert!(
        idx_restart_policy < idx_containers,
        "restartPolicy must be inside spec.template.spec before containers array, got:\n{rows:#?}"
    );

    // 3. spec.template.spec.containers[0].workingDir must be placed alongside name and image, before ports
    let idx_c_image = rows
        .iter()
        .position(|r| r.ends_with(":spec.template.spec.containers[0].image"))
        .expect("container image");
    let idx_c_working_dir = rows
        .iter()
        .position(|r| r.ends_with(":spec.template.spec.containers[0].workingDir"))
        .expect("container workingDir");
    let idx_c_ports = rows
        .iter()
        .position(|r| r.starts_with("arr:") && r.ends_with(":ports"))
        .expect("container ports array");

    assert!(
        idx_c_working_dir > idx_c_image,
        "workingDir should follow container image"
    );
    assert!(
        idx_c_working_dir < idx_c_ports,
        "workingDir must be directly in container[0] before nested ports/resources"
    );
}

#[test]
fn test_rebuild_with_extra_uncollapses_parent_categories_and_focuses() {
    let mut f = FormState::new("Deployment", None, vec![]);
    // Collapse spec section
    let sec_spec_idx = f
        .rows
        .iter()
        .position(|r| matches!(&r.kind, Row::Section(s) if s == "spec"))
        .expect("spec section");
    f.toggle_category_at(sec_spec_idx);
    assert!(f.is_category_collapsed(sec_spec_idx));

    // Now add spec.minReadySeconds
    f.rebuild_with_extra(&["spec.minReadySeconds".to_string()]);

    // Parent spec category should now be automatically uncollapsed so the user sees the field
    assert!(
        !f.is_category_collapsed(sec_spec_idx),
        "spec category must be uncollapsed when adding a field inside it"
    );
    assert_eq!(
        f.focused_field().map(|fld| fld.path.as_str()),
        Some("spec.minReadySeconds"),
        "focus must land on the newly added field"
    );
}

#[test]
fn test_seeded_form_materializes_multi_item_arrays_maps_and_string_lists() {
    let doc = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": {
            "name": "payments-api",
            "annotations": {
                "deployment.kubernetes.io/revision": "3",
                "prometheus.io/scrape": "true"
            }
        },
        "spec": {
            "replicas": 2,
            "template": {
                "spec": {
                    "nodeSelector": {
                        "kubernetes.io/os": "linux"
                    },
                    "containers": [
                        {
                            "name": "app",
                            "image": "payments:1.2.0",
                            "command": ["sh", "-c", "exec /app/server"],
                            "ports": [
                                {"containerPort": 8080},
                                {"containerPort": 9090}
                            ]
                        },
                        {
                            "name": "sidecar",
                            "image": "envoy:1.29",
                            "ports": [
                                {"containerPort": 15001}
                            ]
                        }
                    ]
                }
            }
        }
    });

    let f = FormState::new("Deployment", Some(&doc), vec![]);
    let p = paths(&f);

    // 1. Both containers and both ports must be materialized in the form rows
    assert!(
        p.contains(&"spec.template.spec.containers[0].name"),
        "missing container 0: {p:?}"
    );
    assert!(
        p.contains(&"spec.template.spec.containers[1].name"),
        "missing container 1: {p:?}"
    );
    assert!(
        p.contains(&"spec.template.spec.containers[0].ports[0].containerPort"),
        "missing container 0 port 0: {p:?}"
    );
    assert!(
        p.contains(&"spec.template.spec.containers[0].ports[1].containerPort"),
        "missing container 0 port 1: {p:?}"
    );

    // 2. Map fields with dotted keys (annotations, nodeSelector) must stay single Map fields, not split on dots
    assert!(
        p.contains(&"metadata.annotations"),
        "annotations should be a single Map field: {p:?}"
    );
    assert!(
        !p.iter().any(|x| x.starts_with("metadata.annotations.")),
        "annotations keys must not be split into child schema fields: {p:?}"
    );
    assert!(
        p.contains(&"spec.template.spec.nodeSelector"),
        "nodeSelector should be a single Map field: {p:?}"
    );

    // 3. StringList field (command) renders comma-separated and preserves array structure
    assert!(
        p.contains(&"spec.template.spec.containers[0].command"),
        "command should be materialized as a StringList field: {p:?}"
    );
    let cmd_row = f
        .rows
        .iter()
        .find(|r| matches!(&r.kind, Row::Field { field, .. } if field.path == "spec.template.spec.containers[0].command"))
        .expect("command row");
    assert_eq!(cmd_row.display, "sh, -c, exec /app/server");
}

#[test]
fn test_string_list_coercion_and_yaml_emission() {
    let mut f = FormState::new("Pod", None, vec![]);
    f.rebuild_with_extra(&["spec.containers[].command".to_string()]);
    focus(&mut f, "spec.containers[0].command");
    f.begin_edit();
    for c in "/bin/sh, -c, echo hello".chars() {
        f.insert_char(c);
    }
    f.commit();

    assert_eq!(
        value(&f, "spec.containers[0].command"),
        Some(json!(["/bin/sh", "-c", "echo hello"]))
    );
    let yaml = yamlly_core::emit_yaml_owned(f.data.clone());
    assert!(
        yaml.contains("- /bin/sh") && yaml.contains("- -c") && yaml.contains("- echo hello"),
        "StringList should emit a YAML sequence:\n{yaml}"
    );
}
