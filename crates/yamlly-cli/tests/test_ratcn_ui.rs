use ratatui::Terminal;
use ratatui::backend::TestBackend;
use ratcn::InputState;
use ratcn::runtime::{
    ChildId, Event, FocusState, KeyCode, KeyEvent, Modifiers, MouseButton, MouseEvent, MouseKind,
};
use serde_json::json;
use yamlly::app::{App, Msg};
use yamlly::modals::{
    FIELD_PICKER_GROUP_PREFIX, FieldPickerRow, MODAL_CLUSTER_IMPORT, MODAL_CONFIRM,
    MODAL_CONTEXT_SWITCHER, MODAL_DIR_PICKER, MODAL_FIELD_PICKER, MODAL_MAP_EDITOR, MODAL_PREVIEW,
    MODAL_SELECT_PICKER, SELECT_UNSET_VALUE,
};
use yamlly::ui;

fn render_frame(
    ratcn: &mut ratcn::runtime::Ratcn<App, Msg>,
    app: &mut App,
    w: u16,
    h: u16,
) -> String {
    app.tick_clock();
    let backend = TestBackend::new(w, h);
    let mut terminal = Terminal::new(backend).expect("test terminal");
    terminal
        .draw(|frame| {
            ui::render(ratcn, frame, app);
        })
        .expect("draw frame");
    let buf = terminal.backend().buffer();
    let mut out = String::new();
    for y in 0..h {
        for x in 0..w {
            out.push_str(buf[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

fn send_key(ratcn: &mut ratcn::runtime::Ratcn<App, Msg>, app: &mut App, code: KeyCode) {
    ui::handle_event(ratcn, app, Event::Key(KeyEvent::new(code)));
}

fn send_ctrl(ratcn: &mut ratcn::runtime::Ratcn<App, Msg>, app: &mut App, ch: char) {
    let mut ev = KeyEvent::new(KeyCode::Char(ch));
    ev.modifiers = Modifiers {
        ctrl: true,
        alt: false,
        shift: false,
    };
    ui::handle_event(ratcn, app, Event::Key(ev));
}

fn send_text(ratcn: &mut ratcn::runtime::Ratcn<App, Msg>, app: &mut App, s: &str) {
    for c in s.chars() {
        send_key(ratcn, app, KeyCode::Char(c));
    }
}

fn send_click(ratcn: &mut ratcn::runtime::Ratcn<App, Msg>, app: &mut App, x: u16, y: u16) {
    let down = Event::Mouse(MouseEvent {
        kind: MouseKind::Down(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::default(),
    });
    let up = Event::Mouse(MouseEvent {
        kind: MouseKind::Up(MouseButton::Left),
        column: x,
        row: y,
        modifiers: Modifiers::default(),
    });
    ui::handle_event(ratcn, app, down);
    ui::handle_event(ratcn, app, up);
}

#[test]
fn test_shell_renders_header_sidebar_empty_state_and_footer() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);

    assert!(screen.contains("yamlly"), "missing brand header:\n{screen}");
    assert!(
        screen.contains("Connected: test-cluster [default]"),
        "missing cluster badge:\n{screen}"
    );
    assert!(
        screen.contains("Resources (22)"),
        "missing sidebar resource count:\n{screen}"
    );
    assert!(
        screen.contains("No resource tabs are open yet."),
        "missing designed empty state:\n{screen}"
    );
    assert!(
        screen.contains("+ Deployment") && screen.contains("+ Service"),
        "missing quick-launch buttons:\n{screen}"
    );
}

#[test]
fn test_sidebar_filter_and_open_resource_via_ratcn() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    render_frame(&mut ratcn, &mut app, 120, 36);

    // Press '/' to focus the sidebar filter Input
    send_key(&mut ratcn, &mut app, KeyCode::Char('/'));
    render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(app.focus.contains_path(["sidebar", "filter"]));

    // Type "Deployment" into the ratcn::Input and submit with Enter
    send_text(&mut ratcn, &mut app, "Deployment");
    assert_eq!(app.resource_filter.value(), "Deployment");
    assert_eq!(app.filtered_resources().len(), 1);

    send_key(&mut ratcn, &mut app, KeyCode::Enter);
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);

    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].kind, "Deployment");
    assert!(
        screen.contains("Deployment (apps/v1)"),
        "workspace title should show Deployment (apps/v1):\n{screen}"
    );
    assert!(
        screen.contains("✓ Clean"),
        "freshly opened Deployment tab should show ✓ Clean:\n{screen}"
    );
}

#[test]
fn test_form_input_editing_updates_yaml_and_dirty_state() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    render_frame(&mut ratcn, &mut app, 120, 36);

    // Focus starts on the first field (metadata.name, field-1)
    assert!(app.focus.contains_path(["workspace", "form"]));
    send_text(&mut ratcn, &mut app, "frontend-api");
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);

    assert!(app.tabs[0].dirty, "typing in Input must mark tab dirty");
    assert_eq!(app.tabs[0].label, "Deployment: frontend-api");
    assert!(
        screen.contains("● Modified"),
        "dirty badge must appear in workspace title:\n{screen}"
    );

    let yaml = app.active_yaml().expect("yaml");
    assert!(yaml.contains("name: frontend-api"), "yaml:\n{yaml}");
}

#[test]
fn test_validation_highlights_errors_and_pushes_toast() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource(
        "Deployment",
        Some(&json!({
            "metadata": { "name": "web" },
            "spec": { "replicas": "not-an-int" }
        })),
    );
    render_frame(&mut ratcn, &mut app, 120, 36);

    // Trigger validation with Ctrl+V
    send_ctrl(&mut ratcn, &mut app, 'v');
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);

    assert!(
        screen.contains("must be a number"),
        "inline validation error must render on screen:\n{screen}"
    );
    assert!(
        app.active_toast()
            .is_some_and(|t| t.contains("Validation failed")),
        "toast should announce validation failure"
    );
}

#[test]
fn test_field_picker_modal_adds_schema_fields_and_restores_focus() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Pod", None);
    render_frame(&mut ratcn, &mut app, 120, 36);

    let before_count = app.active_form().unwrap().rows.len();

    // Open field picker with Ctrl+F
    send_ctrl(&mut ratcn, &mut app, 'f');
    assert!(app.modals.is_open(MODAL_FIELD_PICKER));
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen.contains("Add Optional Fields — Pod"),
        "field picker dialog should render:\n{screen}"
    );

    // Filter for hostNetwork in the field picker's search Input
    send_text(&mut ratcn, &mut app, "hostNetwork");
    render_frame(&mut ratcn, &mut app, 120, 36);
    assert_eq!(
        app.field_picker_modal.cursor.as_deref(),
        Some("spec.hostNetwork")
    );

    // Commit selection with Enter
    send_key(&mut ratcn, &mut app, KeyCode::Enter);
    render_frame(&mut ratcn, &mut app, 120, 36);

    assert!(!app.modals.is_open(MODAL_FIELD_PICKER));
    let after_count = app.active_form().unwrap().rows.len();
    assert!(
        after_count > before_count,
        "adding spec.hostNetwork must grow the form rows"
    );
    assert!(
        app.active_form()
            .unwrap()
            .shown_paths
            .contains(&"spec.hostNetwork".to_string())
    );
}

#[test]
fn test_map_editor_modal_edits_key_value_pairs() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    app.open_map_editor("metadata.labels");
    assert!(app.modals.is_open(MODAL_MAP_EDITOR));

    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen.contains("Edit Map — metadata.labels"),
        "map editor dialog should render:\n{screen}"
    );

    // Type key into first row's key Input, Tab to value Input, type value
    app.focus = FocusState::intent([
        ChildId::Static(MODAL_MAP_EDITOR),
        ChildId::Static("scroll"),
        app.map_editor_modal.rows[0].id.clone(),
        ChildId::Static("key"),
    ]);
    render_frame(&mut ratcn, &mut app, 120, 36);
    send_text(&mut ratcn, &mut app, "app");

    send_key(&mut ratcn, &mut app, KeyCode::Tab);
    render_frame(&mut ratcn, &mut app, 120, 36);
    send_text(&mut ratcn, &mut app, "nginx");

    // Apply map changes
    app.update(Msg::MapEditorSave);
    render_frame(&mut ratcn, &mut app, 120, 36);

    assert!(!app.modals.is_open(MODAL_MAP_EDITOR));
    let yaml = app.active_yaml().unwrap();
    assert!(
        yaml.contains("app: nginx"),
        "yaml should contain label:\n{yaml}"
    );
}

#[test]
fn test_select_picker_modal_sets_and_clears_enum_value() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Service", None);
    app.open_select_picker("spec.type");
    assert!(app.modals.is_open(MODAL_SELECT_PICKER));

    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen.contains("ClusterIP") && screen.contains("LoadBalancer"),
        "select picker should list Service types:\n{screen}"
    );

    app.update(Msg::SelectPickerCommit("LoadBalancer".to_string()));
    assert!(!app.modals.is_open(MODAL_SELECT_PICKER));
    assert_eq!(
        app.active_form().unwrap().field_value("spec.type"),
        Some(&json!("LoadBalancer"))
    );

    // Reopen and clear
    app.open_select_picker("spec.type");
    app.update(Msg::SelectPickerClear);
    assert_eq!(app.active_form().unwrap().field_value("spec.type"), None);
}

#[test]
fn test_preview_and_stacked_dir_picker_and_save_to_disk() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource(
        "ConfigMap",
        Some(&json!({
            "metadata": { "name": "app-cfg" }
        })),
    );
    // Make dirty edit
    app.active_form_mut().unwrap().set_map(
        "metadata.labels",
        vec![("env".to_string(), "prod".to_string())],
    );
    app.refresh_tab_state();
    assert!(app.tabs[0].dirty);

    // Open preview modal
    app.open_preview();
    assert!(app.modals.is_open(MODAL_PREVIEW));
    let initial_path = app.preview_modal.path_input.value().to_string();
    assert!(
        yamlly::modals::expand_tilde(&initial_path).is_absolute(),
        "default save path must resolve to an absolute path: {initial_path}"
    );
    if std::env::var_os("HOME").is_some() {
        assert!(
            initial_path.starts_with("~/") || initial_path.starts_with('/'),
            "default save path must use ~ or absolute prefix: {initial_path}"
        );
    }
    assert!(
        !initial_path.starts_with("./"),
        "default save path must not start with ./: {initial_path}"
    );
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen.contains("YAML Preview & Save Manifest"),
        "preview modal should render:\n{screen}"
    );
    assert!(
        screen.contains("kind:") && screen.contains("ConfigMap"),
        "preview should render highlighted YAML:\n{screen}"
    );

    // Stack directory picker on top of preview modal
    app.update(Msg::OpenDirPicker);
    assert!(app.modals.is_open(MODAL_PREVIEW));
    assert!(app.modals.is_open(MODAL_DIR_PICKER));
    let start_dir = app.dir_picker_modal.current_dir.clone();
    assert!(
        start_dir.is_absolute(),
        "dir picker current_dir must be absolute: {}",
        start_dir.display()
    );
    let screen_stacked = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen_stacked.contains("Browse Directory"),
        "stacked dir picker should render on top:\n{screen_stacked}"
    );

    // Navigate up outside current working directory
    app.update(Msg::DirPickerUp);
    assert_ne!(
        app.dir_picker_modal.current_dir, start_dir,
        "DirPickerUp must traverse up to the parent directory outside cwd"
    );
    let parent_dir = app.dir_picker_modal.current_dir.clone();

    // Select current parent directory -> updates preview path to shortened parent_dir/app-cfg-configmap.yaml
    app.update(Msg::DirPickerSelectCurrent);
    assert!(!app.modals.is_open(MODAL_DIR_PICKER));
    assert!(app.modals.is_open(MODAL_PREVIEW));
    assert_eq!(
        app.preview_modal.path_input.value(),
        yamlly::modals::shorten_home_path(&parent_dir.join("app-cfg-configmap.yaml"))
    );

    // Save to a temporary directory anywhere on the system
    let tmp_dir = std::env::temp_dir().join(format!("yamlly-save-test-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&tmp_dir);
    app.update(Msg::PreviewPathChanged(ratcn::InputState::new(
        tmp_dir.to_string_lossy().to_string(),
    )));
    app.update(Msg::PreviewSave);

    assert!(!app.modals.is_open(MODAL_PREVIEW));
    assert!(
        !app.tabs[0].dirty,
        "saving YAML must mark the tab clean again"
    );
    let expected_file = tmp_dir.join("app-cfg-configmap.yaml");
    let saved = std::fs::read_to_string(&expected_file).expect("saved yaml file in tmp_dir");
    assert!(saved.contains("kind: ConfigMap") && saved.contains("env: prod"));
    let _ = std::fs::remove_dir_all(&tmp_dir);
}

#[test]
fn test_confirm_modal_guards_dirty_tab_close_and_quit() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    render_frame(&mut ratcn, &mut app, 120, 36);

    // Make tab dirty
    send_text(&mut ratcn, &mut app, "dirty-deploy");
    assert!(app.tabs[0].dirty);

    // Request close active tab (Ctrl+W) -> opens Confirm dialog instead of closing immediately
    send_ctrl(&mut ratcn, &mut app, 'w');
    assert!(app.modals.is_open(MODAL_CONFIRM));
    assert_eq!(app.tabs.len(), 1);
    let confirm_screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        confirm_screen.contains("Close Unsaved Tab") && confirm_screen.contains("Discard & Close"),
        "confirm dialog should render:\n{confirm_screen}"
    );

    // Dismiss with Esc -> tab stays open
    send_key(&mut ratcn, &mut app, KeyCode::Esc);
    assert!(!app.modals.is_open(MODAL_CONFIRM));
    assert_eq!(app.tabs.len(), 1);

    // Request quit -> opens Confirm dialog; accept -> app.running becomes false
    app.request_quit();
    assert!(app.modals.is_open(MODAL_CONFIRM));
    assert!(app.running);
    app.update(Msg::ConfirmAccepted);
    assert!(!app.running);
}

#[test]
fn test_theme_cycling_and_mouse_click_on_quick_launch() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);

    let initial_theme = app.theme().name;
    app.cycle_theme();
    assert_ne!(app.theme().name, initial_theme);

    // Find "+ Deployment" button coordinates on the rendered screen and click it with the mouse
    let lines: Vec<&str> = screen.lines().collect();
    let (btn_y, btn_x) = lines
        .iter()
        .enumerate()
        .find_map(|(y, line)| {
            line.find("+ Deployment")
                .map(|byte_idx| (y as u16, line[..byte_idx].chars().count() as u16 + 2))
        })
        .expect("find + Deployment button on screen");

    send_click(&mut ratcn, &mut app, btn_x, btn_y);
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].kind, "Deployment");
}

#[test]
fn test_large_fields_buttons_and_spacing_render() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    let screen = render_frame(&mut ratcn, &mut app, 130, 44);
    eprintln!("\n=== SCREEN OUTPUT ===\n{screen}\n=====================");

    // 1. Sidebar search input renders as a 3-row titled box
    assert!(
        screen.contains("Search Resources (/)"),
        "sidebar search input should have a 3-row titled border:\n{screen}"
    );

    // 2. Form fields render as 3-row titled boxes with type badges
    assert!(
        screen.contains("name [text]"),
        "form text input should render 3-row titled border with 'name [text]':\n{screen}"
    );
    assert!(
        screen.contains("namespace [namespace]"),
        "namespace field should render 3-row titled border:\n{screen}"
    );

    // 3. Toolbar buttons render with large 3-row height
    assert!(
        screen.contains("↓ Import")
            && screen.contains("+ Fields")
            && screen.contains("✓ Validate")
            && screen.contains("⎙ Preview & Save"),
        "toolbar buttons should render:\n{screen}"
    );
}

#[test]
fn test_enum_and_namespace_render_as_dropdown_list_and_open_on_enter() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Service", None);
    let screen = render_frame(&mut ratcn, &mut app, 130, 56);

    // Enum field (spec.type) renders as a dropdown trigger button with placeholder + ▾
    assert!(
        screen.contains("Select type... (unset)") && screen.contains("▾"),
        "Enum field should render as a dropdown trigger button:\n{screen}"
    );

    // Focus the spec.type dropdown field and press Enter -> opens SelectPickerModal dropdown list
    let type_row_idx = app
        .active_form()
        .unwrap()
        .rows
        .iter()
        .position(|r| matches!(&r.kind, yamlly_core::field::Row::Field { field, .. } if field.path == "spec.type"))
        .expect("spec.type row");
    app.focus = FocusState::intent([
        ChildId::Static("workspace"),
        ChildId::Static("form"),
        ChildId::from(format!("field-{type_row_idx}")),
    ]);
    render_frame(&mut ratcn, &mut app, 130, 44);

    send_key(&mut ratcn, &mut app, KeyCode::Enter);
    assert!(
        app.modals.is_open(MODAL_SELECT_PICKER),
        "pressing Enter on enum dropdown button must open SelectPickerModal"
    );

    let modal_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        modal_screen.contains("(unset / clear)")
            && modal_screen.contains("ClusterIP")
            && modal_screen.contains("NodePort")
            && modal_screen.contains("LoadBalancer"),
        "dropdown modal must show (unset / clear) and all enum options:\n{modal_screen}"
    );

    // Select LoadBalancer -> modal closes and dropdown button displays LoadBalancer
    app.update(Msg::SelectPickerCommit("LoadBalancer".to_string()));
    assert!(!app.modals.is_open(MODAL_SELECT_PICKER));
    let updated_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        updated_screen.contains("LoadBalancer"),
        "dropdown button should display selected value LoadBalancer:\n{updated_screen}"
    );

    // Selecting SELECT_UNSET_VALUE clears the enum field back to None
    app.open_select_picker("spec.type");
    app.update(Msg::SelectPickerCommit(SELECT_UNSET_VALUE.to_string()));
    assert_eq!(app.active_form().unwrap().field_value("spec.type"), None);
}

#[test]
fn test_cluster_import_modal_imports_labels_selector_and_manifests_like_ktui() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    let screen = render_frame(&mut ratcn, &mut app, 130, 44);

    // Map fields render an inline "↓ Import from Cluster" button next to "✎ Edit Map"
    assert!(
        screen.contains("↓ Import from Cluster"),
        "map fields must show '↓ Import from Cluster' button:\n{screen}"
    );

    // 1. Open ClusterImportModal for metadata.labels
    app.update(Msg::OpenClusterImport("metadata.labels".to_string()));
    assert!(app.modals.is_open(MODAL_CLUSTER_IMPORT));
    let ci_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        ci_screen.contains("Import labels from Cluster — metadata.labels")
            && ci_screen.contains("Search cluster resources")
            && ci_screen.contains("default/web-frontend")
            && ci_screen.contains("default/payments-api"),
        "ClusterImportModal should render filter and list cluster resources:\n{ci_screen}"
    );

    // Test search filter: filter by "pay"
    app.update(Msg::ClusterImportFilterChanged(InputState::new("pay")));
    let filtered_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        filtered_screen.contains("default/payments-api"),
        "filtered screen should show payments-api:\n{filtered_screen}"
    );

    // Commit filtered resource (default/payments-api)
    app.update(Msg::ClusterImportCommitSelected);
    assert!(!app.modals.is_open(MODAL_CLUSTER_IMPORT));
    let yaml_payments = app.active_yaml().unwrap();
    assert!(
        yaml_payments.contains("app: payments-api") && yaml_payments.contains("tier: backend"),
        "metadata.labels should be imported from filtered payments-api Deployment:\n{yaml_payments}"
    );

    // Reopen and test keyboard Up/Down navigation and refresh
    app.update(Msg::OpenClusterImport("metadata.labels".to_string()));
    assert!(app.modals.is_open(MODAL_CLUSTER_IMPORT));
    assert_eq!(app.cluster_import_modal.cursor.as_deref(), Some("0"));
    // Move down
    send_key(&mut ratcn, &mut app, KeyCode::Down);
    assert_eq!(app.cluster_import_modal.cursor.as_deref(), Some("1"));
    // Refresh reloads
    app.update(Msg::ClusterImportRefresh);
    assert!(!app.cluster_import_modal.items.is_empty());

    // Commit selected via Enter key
    send_key(&mut ratcn, &mut app, KeyCode::Enter);
    assert!(!app.modals.is_open(MODAL_CLUSTER_IMPORT));

    // 2. Open ClusterImportModal for spec.selector.matchLabels -> ktui special case extracts spec.template.metadata.labels
    app.update(Msg::OpenClusterImport(
        "spec.selector.matchLabels".to_string(),
    ));
    assert!(app.modals.is_open(MODAL_CLUSTER_IMPORT));
    app.update(Msg::ClusterImportCommitSelected);
    let yaml_after_selector = app.active_yaml().unwrap();
    assert!(
        yaml_after_selector.contains("pod-template: v2"),
        "importing Deployment into selector.matchLabels must extract spec.template.metadata.labels:\n{yaml_after_selector}"
    );

    // 3. Open MapEditorModal and trigger "↓ Import from Cluster" from inside MapEditorModal
    app.open_map_editor("metadata.labels");
    assert!(app.modals.is_open(MODAL_MAP_EDITOR));
    app.update(Msg::OpenClusterImportFromMapEditor);
    assert!(app.modals.is_open(MODAL_CLUSTER_IMPORT));
    // Switch kind tab in ClusterImportModal to Pod
    app.update(Msg::ClusterImportKindSelected("Pod".to_string()));
    app.update(Msg::ClusterImportCommitSelected);
    assert!(!app.modals.is_open(MODAL_CLUSTER_IMPORT));
    assert!(
        app.modals.is_open(MODAL_MAP_EDITOR),
        "MapEditorModal should remain open with merged cluster entries"
    );
    app.update(Msg::MapEditorSave);
    let yaml_after_pod = app.active_yaml().unwrap();
    assert!(
        yaml_after_pod.contains("role: cache"),
        "importing from Pod inside MapEditorModal must merge role=cache:\n{yaml_after_pod}"
    );

    // 4. Import full manifest from cluster via OpenClusterImportManifest
    let tabs_before = app.tabs.len();
    app.update(Msg::OpenClusterImportManifest);
    assert!(app.modals.is_open(MODAL_CLUSTER_IMPORT));
    app.update(Msg::ClusterImportCommitSelected);
    assert_eq!(app.tabs.len(), tabs_before + 1);
    assert_eq!(app.tabs.last().unwrap().label, "Deployment: web-frontend");

    // Verify sanitized manifest: runtime server metadata must be stripped
    let full_yaml = app.active_yaml().unwrap();
    assert!(
        !full_yaml.contains("status:")
            && !full_yaml.contains("1234-abcd-5678")
            && !full_yaml.contains("resourceVersion:")
            && !full_yaml.contains("managedFields:")
            && !full_yaml.contains("last-applied-configuration"),
        "imported manifest must strip server runtime metadata:\n{full_yaml}"
    );
    assert!(
        full_yaml.contains("replicas: 3")
            && full_yaml.contains("name: web-frontend")
            && full_yaml.contains("kubernetes.io/change-cause: rollout-v2"),
        "imported manifest must retain valid spec and metadata:\n{full_yaml}"
    );
}

#[test]
fn test_collapsible_categories_hide_and_restore_nested_rows() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    let screen = render_frame(&mut ratcn, &mut app, 130, 44);

    assert!(
        screen.contains("▾ ── METADATA") && screen.contains("name [text]"),
        "METADATA section and name field should initially be expanded:\n{screen}"
    );

    // Collapse the METADATA section (row 0)
    app.update(Msg::ToggleFormCategory(0));
    let collapsed_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        collapsed_screen.contains("▸ ── METADATA (collapsed)"),
        "METADATA section should show collapsed indicator:\n{collapsed_screen}"
    );
    assert!(
        !collapsed_screen.contains("namespace [namespace]"),
        "fields inside collapsed METADATA section must be hidden:\n{collapsed_screen}"
    );

    // Expand the METADATA section again
    app.update(Msg::ToggleFormCategory(0));
    let expanded_screen = render_frame(&mut ratcn, &mut app, 130, 44);
    assert!(
        expanded_screen.contains("▾ ── METADATA")
            && expanded_screen.contains("namespace [namespace]"),
        "expanding METADATA section must restore child fields:\n{expanded_screen}"
    );

    // Collapse a nested Group (selector) and verify its child (matchLabels) is hidden
    let selector_idx = app
        .active_form()
        .unwrap()
        .rows
        .iter()
        .position(|r| matches!(&r.kind, yamlly_core::field::Row::Group { label, .. } if label == "selector"))
        .expect("selector group row");
    app.update(Msg::ToggleFormCategory(selector_idx));
    let vis = app.active_form().unwrap().row_visibility();
    assert!(
        !vis[selector_idx + 1],
        "child row under collapsed 'selector' group must have visibility=false"
    );
}

#[test]
fn test_field_picker_tree_rendering_selection_and_category_deduplication() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    render_frame(&mut ratcn, &mut app, 130, 44);

    // 1. Open Field Picker
    app.update(Msg::OpenFieldPicker);
    assert!(app.modals.is_open(MODAL_FIELD_PICKER));
    let screen = render_frame(&mut ratcn, &mut app, 130, 44);

    // Verify dialog header and search bar
    assert!(
        screen.contains("Add Optional Fields — Deployment"),
        "field picker dialog must show title:\n{screen}"
    );

    // Verify tree rows contain category branch rows with folder/package icons and arrows
    let rows = app.field_picker_modal.tree_rows();
    let has_category = rows
        .iter()
        .any(|r| matches!(r, FieldPickerRow::Category { .. }));
    assert!(
        has_category,
        "field picker should contain hierarchical category rows"
    );

    // Verify that common fields (e.g. metadata.name, metadata.namespace, spec.replicas) are NOT present
    for common_path in &["metadata.name", "metadata.namespace", "spec.replicas"] {
        let is_present = rows.iter().any(|r| match r {
            FieldPickerRow::Leaf { path, .. } => path == common_path,
            _ => false,
        });
        assert!(
            !is_present,
            "{common_path} is already in the form and must not appear in field picker"
        );
    }

    // Verify that requests and limits maps (already present in common_fields as requests.cpu/requests.memory)
    // are correctly filtered out and do not appear as redundant categories
    let has_requests_leaf = rows.iter().any(|r| match r {
        FieldPickerRow::Leaf { path, .. } => path.contains("resources.requests"),
        _ => false,
    });
    assert!(
        !has_requests_leaf,
        "resources.requests map is already present and must be removed"
    );

    // 2. Test Category Expand and Collapse
    let strategy_id = format!("{FIELD_PICKER_GROUP_PREFIX}spec.strategy");
    app.field_picker_modal.cursor = Some(strategy_id.clone());

    // Initially spec.strategy should be expanded (depth <= 3)
    let has_strategy_type_initial = app.field_picker_modal.tree_rows().iter().any(|r| match r {
        FieldPickerRow::Leaf { path, .. } => path == "spec.strategy.type",
        _ => false,
    });
    assert!(
        has_strategy_type_initial,
        "spec.strategy.type should initially be visible"
    );

    // Collapse spec.strategy
    let collapsed = app.field_picker_modal.expand_or_collapse_cursor(false);
    assert!(collapsed, "collapsing spec.strategy should succeed");
    let has_strategy_type_collapsed = app.field_picker_modal.tree_rows().iter().any(|r| match r {
        FieldPickerRow::Leaf { path, .. } => path == "spec.strategy.type",
        _ => false,
    });
    assert!(
        !has_strategy_type_collapsed,
        "spec.strategy.type must be hidden when spec.strategy is collapsed"
    );

    // Expand spec.strategy again
    let expanded = app.field_picker_modal.expand_or_collapse_cursor(true);
    assert!(expanded, "expanding spec.strategy should succeed");
    let has_strategy_type_reexpanded = app.field_picker_modal.tree_rows().iter().any(|r| match r {
        FieldPickerRow::Leaf { path, .. } => path == "spec.strategy.type",
        _ => false,
    });
    assert!(
        has_strategy_type_reexpanded,
        "spec.strategy.type must be visible after expanding"
    );

    // 3. Test Category Selection (Group toggle via 'a' / Msg::FieldPickerToggleGroup)
    app.field_picker_modal.cursor = Some(strategy_id.clone());
    app.update(Msg::FieldPickerToggleGroup);
    assert!(
        app.field_picker_modal
            .selected
            .contains("spec.strategy.type"),
        "toggling group selection on spec.strategy must select spec.strategy.type"
    );

    // 4. Commit and add the entire strategy category to the form
    let before_rows = app.active_form().unwrap().rows.len();
    app.update(Msg::FieldPickerCommit);
    assert!(!app.modals.is_open(MODAL_FIELD_PICKER));
    let after_rows = app.active_form().unwrap().rows.len();
    assert!(
        after_rows > before_rows,
        "adding spec.strategy category must expand form rows"
    );
    assert!(
        app.active_form()
            .unwrap()
            .shown_paths
            .contains(&"spec.strategy.type".to_string()),
        "form shown_paths must now contain spec.strategy.type"
    );

    // 5. Test Deduplication: Re-open field picker and verify spec.strategy category is GONE
    app.update(Msg::OpenFieldPicker);
    assert!(app.modals.is_open(MODAL_FIELD_PICKER));
    let new_rows = app.field_picker_modal.tree_rows();
    let has_strategy_category = new_rows.iter().any(|r| match r {
        FieldPickerRow::Category { group_path, .. } => group_path == "spec.strategy",
        _ => false,
    });
    assert!(
        !has_strategy_category,
        "spec.strategy category cannot appear more than once and must be removed after being added"
    );

    // Close modal
    app.update(Msg::CloseModal);

    // 6. Test Initial Data Seeding & Deduplication:
    // Opening a document with existing custom fields (e.g. strategy) seeds shown_paths and excludes them from the picker
    let doc = json!({
        "apiVersion": "apps/v1",
        "kind": "Deployment",
        "metadata": { "name": "existing-deploy" },
        "spec": {
            "strategy": {
                "type": "Recreate"
            }
        }
    });
    app.open_resource("Deployment", Some(&doc));
    let active_f = app.active_form().unwrap();
    assert!(
        active_f
            .shown_paths
            .iter()
            .any(|p| p.starts_with("spec.strategy")),
        "initial document must seed shown_paths with spec.strategy"
    );

    // Open field picker for this newly opened tab
    app.update(Msg::OpenFieldPicker);
    let seeded_picker_rows = app.field_picker_modal.tree_rows();
    let strategy_in_seeded = seeded_picker_rows.iter().any(|r| match r {
        FieldPickerRow::Category { group_path, .. } => group_path == "spec.strategy",
        FieldPickerRow::Leaf { path, .. } => path.starts_with("spec.strategy"),
    });
    assert!(
        !strategy_in_seeded,
        "spec.strategy is already in the document and must not appear in the field picker"
    );
}

#[test]
fn test_per_tab_close_button_and_tab_switching() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();

    // 1. Open two tabs: Deployment and Service
    app.open_resource("Deployment", None);
    app.open_resource("Service", None);
    assert_eq!(app.tabs.len(), 2);
    assert_eq!(app.active_tab_idx, Some(1)); // Service is active

    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    // Tab strip renders both tab titles and the '✕' close buttons
    assert!(
        screen.contains("Deployment") && screen.contains("Service"),
        "both tabs should appear:\n{screen}"
    );
    assert!(
        screen.contains("✕"),
        "tab close buttons must render:\n{screen}"
    );

    // 2. Click tab 0 ("Deployment") to activate it
    let tab0_id = app.tabs[0].id.clone();
    let tab1_id = app.tabs[1].id.clone();
    app.update(Msg::TabSelected(tab0_id.clone()));
    assert_eq!(app.active_tab_idx, Some(0));

    // 3. Click the close button '✕' on tab 1 ("Service")
    app.update(Msg::CloseTabById(tab1_id));
    assert_eq!(app.tabs.len(), 1);
    assert_eq!(app.tabs[0].id, tab0_id);

    // 4. Dirty tab close guards with confirm modal
    send_text(&mut ratcn, &mut app, "my-dirty-deploy");
    assert!(app.tabs[0].dirty);

    // Clicking close on the dirty tab triggers confirm modal
    app.update(Msg::CloseTabById(tab0_id));
    assert!(
        app.modals.is_open(MODAL_CONFIRM),
        "dirty tab close via '✕' button must open confirm modal"
    );
    assert_eq!(app.tabs.len(), 1);

    // Accept discard & close
    app.update(Msg::ConfirmAccepted);
    assert_eq!(app.tabs.len(), 0, "confirming discard must close the tab");
}

#[test]
fn test_context_switcher_modal_opens_navigates_and_switches_context() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();

    // 1. Initially on test-cluster
    assert_eq!(
        app.cluster_status.context_name.as_deref(),
        Some("test-cluster")
    );

    // 2. Open via single-key 'c' shortcut
    send_key(&mut ratcn, &mut app, KeyCode::Char('c'));
    assert!(
        app.modals.is_open(MODAL_CONTEXT_SWITCHER),
        "pressing 'c' must open the context switcher modal"
    );

    // 3. Render screen and verify title and current context marker
    let screen = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen.contains("Switch Kubernetes Context"),
        "modal must render title 'Switch Kubernetes Context':\n{screen}"
    );
    assert!(
        screen.contains("Search contexts"),
        "modal must render titled filter input:\n{screen}"
    );
    assert!(
        screen.contains("● test-cluster (current)"),
        "active context must be tagged with (current):\n{screen}"
    );

    // 4. Populate with multiple contexts for navigation & filtering test
    app.context_switcher_modal = yamlly::modals::ContextSwitcherModalState::open_for(
        Some("test-cluster".to_string()),
        vec![
            "minikube".to_string(),
            "production-east".to_string(),
            "staging-west".to_string(),
            "test-cluster".to_string(),
        ],
    );

    // Filter contexts by typing "prod"
    send_text(&mut ratcn, &mut app, "prod");
    assert_eq!(app.context_switcher_modal.filter.value(), "prod");
    assert_eq!(
        app.context_switcher_modal.filtered_contexts(),
        vec!["production-east"]
    );

    let screen_filtered = render_frame(&mut ratcn, &mut app, 120, 36);
    assert!(
        screen_filtered.contains("production-east"),
        "filtered list should show production-east:\n{screen_filtered}"
    );
    assert!(
        !screen_filtered.contains("minikube"),
        "minikube should be filtered out:\n{screen_filtered}"
    );

    // 5. Select production-east and commit
    app.update(Msg::ContextSwitcherCommit("production-east".to_string()));
    assert!(
        !app.modals.is_open(MODAL_CONTEXT_SWITCHER),
        "committing must close the context switcher modal"
    );
    assert_eq!(
        app.cluster_status.context_name.as_deref(),
        Some("production-east")
    );

    // 6. Global shortcut Ctrl+K also opens context switcher
    send_ctrl(&mut ratcn, &mut app, 'k');
    assert!(
        app.modals.is_open(MODAL_CONTEXT_SWITCHER),
        "Ctrl+K must open the context switcher modal"
    );
}

#[test]
fn test_bigger_text_fields_and_modal_widths() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);

    // 1. Verify header renders Context button when width is >= 130
    let wide_screen = render_frame(&mut ratcn, &mut app, 130, 40);
    assert!(
        wide_screen.contains("⎈ test-cluster") || wide_screen.contains("⎈ Context"),
        "header should contain kube context button:\n{wide_screen}"
    );
    assert!(
        wide_screen.contains("┌ name [text]")
            && wide_screen.contains("  Enter value...")
            && wide_screen.contains("└───────"),
        "form text field should render as a 3-row titled box with title, input, and bottom border:\n{wide_screen}"
    );

    // 2. Open Map Editor modal on labels and verify generous width for key/value text fields
    app.update(Msg::OpenMapEditor("metadata.labels".to_string()));
    assert!(app.modals.is_open(MODAL_MAP_EDITOR));

    let map_screen = render_frame(&mut ratcn, &mut app, 130, 40);
    assert!(
        map_screen.contains("Edit Map — metadata.labels"),
        "map editor modal should render with wide title:\n{map_screen}"
    );
    assert!(
        map_screen.contains("Key #1") && map_screen.contains("Value #1"),
        "map editor should render large 3-row titled key and value inputs:\n{map_screen}"
    );

    app.update(Msg::CloseModal);
    assert!(!app.modals.is_open(MODAL_MAP_EDITOR));

    // 3. Open Preview & Save modal and verify wide path input
    app.update(Msg::OpenPreview);
    assert!(app.modals.is_open(MODAL_PREVIEW));

    let prev_screen = render_frame(&mut ratcn, &mut app, 130, 40);
    assert!(
        prev_screen.contains("Destination file path"),
        "preview modal must render large 3-row destination file path input:\n{prev_screen}"
    );
}

#[test]
fn test_category_divider_hierarchy_and_input_indentation() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    let screen = render_frame(&mut ratcn, &mut app, 130, 80);

    // 1. Verify root section has horizontal rule
    assert!(
        screen.contains("▾ ── METADATA ──") && screen.contains("▾ ── SPEC ──"),
        "root sections must render full horizontal rule dividers:\n{screen}"
    );

    // 2. Verify subcategories (selector, template, metadata) render with horizontal rules
    assert!(
        screen.contains("▾ ── selector ──")
            && screen.contains("▾ ── template ──")
            && screen.contains("▾ ── metadata ──"),
        "subcategories must render with horizontal rule dividers:\n{screen}"
    );

    // 3. Verify leaf fields are cleanly indented inside their categories
    assert!(
        screen.contains("name [text]")
            && screen.contains("replicas [integer]")
            && screen.contains("matchLabels [map]"),
        "leaf fields must render inside their respective categories:\n{screen}"
    );

    // 4. Verify text input has internal padding (not jammed against border)
    assert!(
        screen.contains("  Enter value..."),
        "text input must have internal padding:\n{screen}"
    );
}

#[test]
fn test_remove_added_fields_categories_and_array_items() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();
    app.open_resource("Deployment", None);
    render_frame(&mut ratcn, &mut app, 130, 80);

    // 1. Default common fields cannot be removed
    let base_rows = app.active_form().unwrap().rows.len();
    assert!(!app.active_form().unwrap().is_added_field("metadata.name"));
    app.update(Msg::RemoveField("metadata.name".to_string()));
    assert_eq!(
        app.active_form().unwrap().rows.len(),
        base_rows,
        "default common fields must not be removed"
    );

    // 2. Add an optional field (`spec.minReadySeconds`) via Field Picker
    app.update(Msg::OpenFieldPicker);
    app.update(Msg::FieldPickerToggle("spec.minReadySeconds".to_string()));
    app.update(Msg::FieldPickerCommit);

    assert!(
        app.active_form()
            .unwrap()
            .is_added_field("spec.minReadySeconds"),
        "spec.minReadySeconds must be marked as an added field"
    );
    let min_ready_idx = app
        .active_form()
        .unwrap()
        .rows
        .iter()
        .position(|r| match &r.kind {
            yamlly_core::Row::Field { field, .. } => field.path == "spec.minReadySeconds",
            _ => false,
        })
        .expect("spec.minReadySeconds row must exist");

    // Set a value on the added field and verify it appears in YAML
    app.update(Msg::FormInputChanged {
        row_idx: min_ready_idx,
        state: InputState::new("15"),
    });
    let yaml_with_field = app.active_yaml().unwrap();
    assert!(
        yaml_with_field.contains("minReadySeconds: 15"),
        "added field value must appear in YAML:\n{yaml_with_field}"
    );

    // Focus the added field and remove it with Ctrl+D
    app.active_form_mut().unwrap().focus = min_ready_idx;
    send_ctrl(&mut ratcn, &mut app, 'd');
    render_frame(&mut ratcn, &mut app, 130, 80);

    assert!(
        !app.active_form()
            .unwrap()
            .shown_paths
            .contains(&"spec.minReadySeconds".to_string()),
        "removed field must no longer be in shown_paths"
    );
    let yaml_after_rm = app.active_yaml().unwrap();
    assert!(
        !yaml_after_rm.contains("minReadySeconds"),
        "removed field value must be deleted from YAML:\n{yaml_after_rm}"
    );

    // Re-open Field Picker and verify `spec.minReadySeconds` is available to add again
    app.update(Msg::OpenFieldPicker);
    assert!(
        app.field_picker_modal
            .available
            .iter()
            .any(|l| l.path == "spec.minReadySeconds"),
        "removed field must become available again in the field picker"
    );
    app.update(Msg::CloseModal);

    // 3. Add an optional category (`spec.strategy`) and remove the entire category
    app.update(Msg::OpenFieldPicker);
    app.field_picker_modal.cursor = Some(format!("{FIELD_PICKER_GROUP_PREFIX}spec.strategy"));
    app.update(Msg::FieldPickerToggleGroup);
    app.update(Msg::FieldPickerCommit);

    let strategy_grp_idx = app
        .active_form()
        .unwrap()
        .rows
        .iter()
        .position(|r| match &r.kind {
            yamlly_core::Row::Group { label, .. } => label == "strategy",
            _ => false,
        })
        .expect("strategy group row must exist");
    assert!(
        app.active_form()
            .unwrap()
            .is_added_category_at(strategy_grp_idx),
        "strategy group must be recognized as an added category"
    );

    let screen_with_strategy = render_frame(&mut ratcn, &mut app, 130, 80);
    assert!(
        screen_with_strategy.contains("✕ Remove"),
        "added category header must render ✕ Remove button:\n{screen_with_strategy}"
    );

    app.update(Msg::RemoveCategory(strategy_grp_idx));
    assert!(
        !app.active_form()
            .unwrap()
            .shown_paths
            .iter()
            .any(|p| p.starts_with("spec.strategy")),
        "removing added category must remove all its child paths"
    );

    // 4. Add an array item to `spec.template.spec.containers[]` and remove it
    app.update(Msg::AddArrayItemAt(
        "spec.template.spec.containers[]".to_string(),
    ));
    let screen_two_items = render_frame(&mut ratcn, &mut app, 130, 80);
    assert!(
        screen_two_items.contains("containers [2 items]")
            && screen_two_items.contains("- Remove item"),
        "array with 2 items must show '- Remove item' button:\n{screen_two_items}"
    );

    app.update(Msg::RemoveArrayItemAt(
        "spec.template.spec.containers[]".to_string(),
    ));
    let screen_one_item = render_frame(&mut ratcn, &mut app, 130, 80);
    assert!(
        screen_one_item.contains("containers [1 item]"),
        "removing array item must return count to 1:\n{screen_one_item}"
    );
}

#[test]
fn test_bottom_bar_buttons_are_clickable() {
    let mut ratcn = ui::new_runtime();
    let mut app = App::new_test();

    // 1. In empty state, click "Open Resource" on the bottom bar
    let screen = render_frame(&mut ratcn, &mut app, 140, 36);
    let footer_y = 35u16;
    let footer_line = screen.lines().nth(footer_y as usize).expect("footer line");
    let open_x = footer_line
        .find("Open Resource")
        .map(|b| footer_line[..b].chars().count() as u16)
        .expect("find Open Resource in footer");
    send_click(&mut ratcn, &mut app, open_x, footer_y);
    assert_eq!(
        app.tabs.len(),
        1,
        "clicking 'Open Resource' in bottom bar must open a tab"
    );

    // 2. With a tab open, click "Preview & Save" on the bottom bar
    let screen_tab = render_frame(&mut ratcn, &mut app, 140, 36);
    let footer_line_tab = screen_tab
        .lines()
        .nth(footer_y as usize)
        .expect("footer line");
    let preview_x = footer_line_tab
        .find("Preview & Save")
        .map(|b| footer_line_tab[..b].chars().count() as u16)
        .expect("find Preview & Save in footer");
    send_click(&mut ratcn, &mut app, preview_x, footer_y);
    assert!(
        app.modals.is_open(MODAL_PREVIEW),
        "clicking 'Preview & Save' in bottom bar must open the preview modal"
    );

    // 3. While modal is open, click "Close Modal" on the bottom bar
    let screen_modal = render_frame(&mut ratcn, &mut app, 140, 36);
    let footer_line_modal = screen_modal
        .lines()
        .nth(footer_y as usize)
        .expect("footer line");
    let close_x = footer_line_modal
        .find("Close Modal")
        .map(|b| footer_line_modal[..b].chars().count() as u16)
        .expect("find Close Modal in footer");
    send_click(&mut ratcn, &mut app, close_x, footer_y);
    assert!(
        !app.modals.is_open(MODAL_PREVIEW),
        "clicking 'Close Modal' in bottom bar must close the active modal"
    );

    // 4. Click "Theme" on the bottom bar
    let prev_theme = app.theme().name;
    let screen_after = render_frame(&mut ratcn, &mut app, 140, 36);
    let footer_line_after = screen_after
        .lines()
        .nth(footer_y as usize)
        .expect("footer line");
    let theme_x = footer_line_after
        .find("Theme")
        .map(|b| footer_line_after[..b].chars().count() as u16)
        .expect("find Theme in footer");
    send_click(&mut ratcn, &mut app, theme_x, footer_y);
    assert_ne!(
        app.theme().name,
        prev_theme,
        "clicking 'Theme' in bottom bar must cycle the theme"
    );
}
