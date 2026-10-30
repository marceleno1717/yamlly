use ratcn::InputState;
use yamlly::app::{App, Msg};
use yamlly::config::EditMode;

#[test]
fn test_basic_mode_default_and_toggle() {
    let mut app = App::new_test();
    assert_eq!(app.user_config.mode, EditMode::Express);

    // Opening Deployment in Express mode should show minimal 5 fields
    app.open_resource("Deployment", None);
    let tab = app.active_tab().unwrap();
    assert!(tab.form.express_mode);

    let field_count = tab
        .form
        .rows
        .iter()
        .filter(|r| matches!(r.kind, yamlly_core::field::Row::Field { .. }))
        .count();
    assert_eq!(field_count, 5); // name, namespace, replicas, image, port

    // Press 'm' to toggle to Advanced mode
    app.update(Msg::ToggleMode);
    assert_eq!(app.user_config.mode, EditMode::Advanced);
    let tab = app.active_tab().unwrap();
    assert!(!tab.form.express_mode);

    let adv_count = tab
        .form
        .rows
        .iter()
        .filter(|r| matches!(r.kind, yamlly_core::field::Row::Field { .. }))
        .count();
    assert!(adv_count >= 15);

    // Toggle back to Express mode
    app.update(Msg::ToggleMode);
    assert_eq!(app.user_config.mode, EditMode::Express);
}

#[test]
fn test_auto_linking_metadata_name_propagates_and_unlinks_on_edit() {
    let mut app = App::new_test();
    app.open_resource("Deployment", None);

    // Type "checkout-service" into metadata.name (row 0)
    let row_idx = app
        .active_form()
        .unwrap()
        .rows
        .iter()
        .position(|r| match &r.kind {
            yamlly_core::field::Row::Field { field, .. } => field.path == "metadata.name",
            _ => false,
        })
        .unwrap();

    app.update(Msg::FormInputChanged {
        row_idx,
        state: InputState::new("checkout-service"),
    });

    let tab = app.active_tab().unwrap();
    assert_eq!(
        yamlly_core::get_nested(&tab.form.data, &["metadata", "labels", "app"])
            .and_then(|v| v.as_str()),
        Some("checkout-service")
    );
    assert_eq!(
        yamlly_core::get_nested(&tab.form.data, &["spec", "selector", "matchLabels", "app"])
            .and_then(|v| v.as_str()),
        Some("checkout-service")
    );
    assert_eq!(
        yamlly_core::get_nested(
            &tab.form.data,
            &["spec", "template", "spec", "containers", "0", "name"]
        )
        .and_then(|v| v.as_str()),
        Some("checkout-service")
    );

    // Now manually edit container name
    let tab_mut = app.active_tab_mut().unwrap();
    tab_mut
        .link_tracker
        .unlink("spec.template.spec.containers[0].name");

    // Change metadata.name again
    app.update(Msg::FormInputChanged {
        row_idx,
        state: InputState::new("billing-service"),
    });

    let tab = app.active_tab().unwrap();
    // Labels follow new name
    assert_eq!(
        yamlly_core::get_nested(&tab.form.data, &["metadata", "labels", "app"])
            .and_then(|v| v.as_str()),
        Some("billing-service")
    );
    // Unlinked container name preserves previous decoupled value
    assert_eq!(
        yamlly_core::get_nested(
            &tab.form.data,
            &["spec", "template", "spec", "containers", "0", "name"]
        )
        .and_then(|v| v.as_str()),
        Some("checkout-service")
    );
}

#[test]
fn test_quick_add_modal_mounts_tolerations_preset() {
    let mut app = App::new_test();
    app.open_resource("Deployment", None);

    assert_eq!(
        app.active_form()
            .unwrap()
            .rows
            .iter()
            .filter(|r| matches!(r.kind, yamlly_core::field::Row::Field { .. }))
            .count(),
        5
    );

    // Open Quick Add modal
    app.update(Msg::OpenQuickAdd);
    assert!(app.modals.is_open(yamlly::modals::MODAL_QUICK_ADD));

    // Preset 0 is Tolerations
    app.update(Msg::QuickAddCommit);
    assert!(!app.modals.is_open(yamlly::modals::MODAL_QUICK_ADD));

    // Form now contains tolerations paths
    let has_tolerations = app
        .active_form()
        .unwrap()
        .shown_paths
        .iter()
        .any(|p| p.contains("tolerations"));
    assert!(
        has_tolerations,
        "tolerations paths must be mounted into form"
    );
}

#[test]
fn test_quick_add_modal_mouse_selection_and_keyboard_navigation() {
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    app.update(Msg::OpenQuickAdd);

    // Initial preset selection should be the first preset
    assert_eq!(app.quick_add_modal.selected_idx, 0);
    assert!(app.quick_add_modal.cursor.is_some());

    // Navigate with keyboard (down)
    app.quick_add_modal.select_next(true);
    assert_eq!(app.quick_add_modal.selected_idx, 1);

    // Mouse selection on resources preset
    app.update(Msg::QuickAddSelect("resources".to_string()));
    assert!(!app.modals.is_open(yamlly::modals::MODAL_QUICK_ADD));

    let has_resources = app
        .active_form()
        .unwrap()
        .shown_paths
        .iter()
        .any(|p| p.contains("resources"));
    assert!(
        has_resources,
        "resources preset paths must be mounted into form"
    );
}

#[test]
fn test_many_tabs_navigation_and_cycling() {
    let mut app = App::new_test();
    let kinds = [
        "Deployment",
        "Service",
        "Pod",
        "ConfigMap",
        "Secret",
        "Ingress",
        "Job",
        "DaemonSet",
    ];
    for k in kinds {
        app.open_resource(k, None);
    }
    assert_eq!(app.tabs.len(), 8);
    assert_eq!(app.active_tab_idx, Some(7));

    // Cycle forward wraps to tab 0
    app.update(Msg::CycleTab(true));
    assert_eq!(app.active_tab_idx, Some(0));

    // Cycle backward wraps back to tab 7
    app.update(Msg::CycleTab(false));
    assert_eq!(app.active_tab_idx, Some(7));

    // Cycle backward to tab 6
    app.update(Msg::CycleTab(false));
    assert_eq!(app.active_tab_idx, Some(6));
}
