use std::collections::HashMap;
use std::path::Path;
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use ratcn::runtime::{ChildId, Event, FocusState, KeyCode, KeyEvent, ModalState};
use ratcn::{InputState, Theme, Toast, ToastKind, ToasterState};
use yamlly_core::ResourceIndexEntry;
use yamlly_core::field::Row;

use crate::form::FormState;
use crate::k8s::{
    ClusterFetchResult, ClusterStatus, KubectlOpKind, KubectlOpResult, check_connection_async,
    clean_cluster_manifest, extract_map_from_cluster_item, fetch_cluster_resources_async,
    resolve_manifest_namespace, run_kubectl_stdin_async,
};
use crate::modals::{
    ClusterImportModalState, ConfirmKind, ConfirmModalState, ContextSwitcherModalState,
    DirPickerModalState, FieldPickerModalState, MODAL_CLUSTER_IMPORT, MODAL_CONFIRM,
    MODAL_CONTEXT_SWITCHER, MODAL_DIR_PICKER, MODAL_FIELD_PICKER, MODAL_MAP_EDITOR, MODAL_PREVIEW,
    MODAL_SELECT_PICKER, MapEditorModalState, PreviewModalState, PreviewViewMode,
    SELECT_UNSET_VALUE, SelectPickerModalState,
};

#[derive(Debug)]
pub struct TabInfo {
    pub id: String,
    pub label: String,
    pub default_label: String,
    pub kind: String,
    pub dirty: bool,
    /// Optional path to the file this tab was loaded from or last saved to.
    pub source_path: Option<String>,
    /// The resource form being edited in this tab.
    pub form: FormState,
}

/// Messages emitted by `ratcn` interactive components and global keybindings.
#[derive(Clone)]
pub enum Msg {
    FocusChanged(FocusState),
    CycleTheme,
    Reconnect,
    // Sidebar resource list & filter
    ResourceFilterChanged(InputState),
    ResourceFilterSubmit,
    ResourceFocusChanged(String, usize),
    ResourceSelected(String),
    // Tabs & workspace toolbar
    TabSelected(String),
    CloseTabById(String),
    CloseActiveTab,
    CycleTab(bool),
    ValidateForm,
    OpenPreview,
    OpenFieldPicker,
    OpenMapEditor(String),
    OpenSelectPicker(String),
    OpenClusterImport(String),
    OpenClusterImportFromMapEditor,
    OpenClusterImportManifest,
    AddArrayItemAt(String),
    RemoveArrayItemAt(String),
    RemoveField(String),
    RemoveFocusedField,
    RemoveCategory(usize),
    ToggleFormCategory(usize),
    // Active form controls
    FormScrollChanged(u16),
    FormInputChanged { row_idx: usize, state: InputState },
    FormInputSubmit { row_idx: usize },
    FormBooleanChanged { row_idx: usize, checked: bool },
    FormOptionCycleChanged { row_idx: usize, option_idx: usize },
    // Modal layer actions
    CloseModal,
    ConfirmAccepted,
    // Field picker modal
    FieldPickerFilterChanged(InputState),
    FieldPickerCursorChanged(String, usize),
    FieldPickerToggle(String),
    FieldPickerToggleGroup,
    FieldPickerCommit,
    // YAML Preview & Save modal
    PreviewScrollChanged(u16),
    PreviewDiffScrollChanged(u16),
    PreviewPathChanged(InputState),
    PreviewSave,
    PreviewDryRun,
    PreviewDiff,
    PreviewApply,
    PreviewToggleView(Option<PreviewViewMode>),
    OpenDirPicker,
    // Directory Picker modal
    DirPickerCursorChanged(String, usize),
    DirPickerEnterDir(String),
    DirPickerUp,
    DirPickerSelectCurrent,
    // Map Editor modal
    MapEditorScrollChanged(u16),
    MapEditorKeyChanged { row_idx: usize, state: InputState },
    MapEditorValueChanged { row_idx: usize, state: InputState },
    MapEditorAddRow,
    MapEditorRemoveRow(usize),
    MapEditorSave,
    // Select Picker (Dropdown List) modal
    SelectPickerCursorChanged(String, usize),
    SelectPickerCommit(String),
    SelectPickerClear,
    // Cluster Import modal
    ClusterImportKindSelected(String),
    ClusterImportFilterChanged(InputState),
    ClusterImportRefresh,
    ClusterImportNamespaceOpen(bool),
    ClusterImportNamespaceSelected(String),
    ClusterImportNamespaceCursorChanged(String),
    ClusterImportCursorChanged(String, usize),
    ClusterImportCommit(String),
    ClusterImportCommitSelected,
    // Context Switcher modal
    OpenContextSwitcher,
    ContextSwitcherFilterChanged(InputState),
    ContextSwitcherCursorChanged(String, usize),
    ContextSwitcherCommit(String),
    ContextSwitcherCommitSelected,
    // App lifecycle
    RequestQuit,
}

pub struct App {
    pub running: bool,
    pub focus: FocusState,
    pub modals: ModalState,
    pub toasts: ToasterState<'static>,
    pub start_instant: Instant,
    pub clock: Duration,
    pub last_toast: Option<(String, Instant)>,
    pub theme_idx: usize,
    pub themes: Vec<Theme>,
    pub resources: Vec<ResourceIndexEntry>,
    pub resource_filter: InputState,
    pub selected_resource_idx: usize,
    pub focused_resource_kind: Option<String>,
    pub cluster_status: ClusterStatus,
    pub cluster_namespaces: Vec<String>,
    /// Default namespace for cluster import queries (persists across modal openings until app exit).
    pub cluster_import_namespace: String,
    pub cluster_rx: Option<Receiver<ClusterStatus>>,
    pub cluster_fetch_rx: Option<Receiver<ClusterFetchResult>>,
    pub kubectl_op_rx: Option<Receiver<KubectlOpResult>>,
    pub last_kubectl_op: Option<KubectlOpResult>,
    pub mock_cluster_resources: Option<HashMap<String, Vec<serde_json::Value>>>,
    pub tabs: Vec<TabInfo>,
    pub active_tab_idx: Option<usize>,
    pub tab_seq: usize,
    pub kind_seq: HashMap<String, usize>,
    // Modal states
    pub confirm_modal: ConfirmModalState,
    pub field_picker_modal: FieldPickerModalState,
    pub preview_modal: PreviewModalState,
    pub dir_picker_modal: DirPickerModalState,
    pub map_editor_modal: MapEditorModalState,
    pub select_picker_modal: SelectPickerModalState,
    pub cluster_import_modal: ClusterImportModalState,
    pub context_switcher_modal: ContextSwitcherModalState,
    // Fallback empty input state for borrowing when no tab/row exists
    pub empty_input: InputState,
    /// Last rendered footer geometry `(x, y, width)` for bottom-bar mouse hit-testing.
    pub footer_rect: std::cell::Cell<(u16, u16, u16)>,
    pub ui_dirty: bool,
}

impl App {
    pub fn new(initial_file: Option<String>) -> Self {
        let themes = Theme::presets().to_vec();
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        let theme_idx = if no_color {
            themes
                .iter()
                .position(|t| t.name == "Terminal")
                .unwrap_or(0)
        } else {
            0
        };
        let resources = yamlly_core::get_all_resources().to_vec();
        let focused_resource_kind = resources.first().map(|r| r.kind.clone());

        let (tx, rx) = channel();
        check_connection_async(tx);

        let mut app = Self {
            running: true,
            focus: FocusState::intent(["sidebar", "resources"]),
            modals: ModalState::default(),
            toasts: ToasterState::new(),
            start_instant: Instant::now(),
            clock: Duration::ZERO,
            last_toast: None,
            theme_idx,
            themes,
            resources,
            resource_filter: InputState::default(),
            selected_resource_idx: 0,
            focused_resource_kind,
            cluster_status: ClusterStatus::default(),
            cluster_namespaces: vec![
                "default".to_string(),
                "kube-system".to_string(),
                "kube-public".to_string(),
            ],
            cluster_import_namespace: "default".to_string(),
            cluster_rx: Some(rx),
            cluster_fetch_rx: None,
            kubectl_op_rx: None,
            last_kubectl_op: None,
            mock_cluster_resources: None,
            tabs: Vec::new(),
            active_tab_idx: None,
            tab_seq: 0,
            kind_seq: HashMap::new(),
            confirm_modal: ConfirmModalState::default(),
            field_picker_modal: FieldPickerModalState::default(),
            preview_modal: PreviewModalState::default(),
            dir_picker_modal: DirPickerModalState::default(),
            map_editor_modal: MapEditorModalState::default(),
            select_picker_modal: SelectPickerModalState::default(),
            cluster_import_modal: ClusterImportModalState::default(),
            context_switcher_modal: ContextSwitcherModalState::default(),
            empty_input: InputState::default(),
            footer_rect: std::cell::Cell::new((0, u16::MAX, 0)),
            ui_dirty: true,
        };

        if let Some(file_path) = initial_file {
            app.load_file(&file_path);
        }

        app
    }

    /// Load one or more Kubernetes resource manifests from a YAML or JSON file on disk
    /// and open an editor tab for each supported resource.
    pub fn load_file(&mut self, file_path: &str) -> usize {
        let content = match std::fs::read_to_string(file_path) {
            Ok(c) => c,
            Err(e) => {
                self.notify_kind(
                    format!("Could not read '{file_path}': {e}"),
                    ToastKind::Error,
                );
                return 0;
            }
        };

        let docs = match yamlly_core::parse_yaml_documents(&content) {
            Ok(d) => d,
            Err(e) => {
                self.notify_kind(
                    format!("Failed to parse '{file_path}': {e}"),
                    ToastKind::Error,
                );
                return 0;
            }
        };

        let mut items = Vec::new();
        for doc in docs {
            if doc.get("kind").and_then(|k| k.as_str()) == Some("List")
                && let Some(arr) = doc.get("items").and_then(|i| i.as_array())
            {
                items.extend(arr.iter().cloned());
            } else {
                items.push(doc);
            }
        }

        let single_doc = items.len() == 1;
        let mut opened = 0usize;
        for raw_doc in &items {
            let Some(raw_kind) = raw_doc.get("kind").and_then(|k| k.as_str()) else {
                continue;
            };
            let canonical_kind = self
                .resources
                .iter()
                .find(|r| r.kind.eq_ignore_ascii_case(raw_kind))
                .map(|r| r.kind.clone())
                .unwrap_or_else(|| raw_kind.to_string());

            if yamlly_core::load_resource_ref(&canonical_kind).is_none() {
                self.notify_kind(
                    format!("Unsupported resource kind '{raw_kind}' in '{file_path}'"),
                    ToastKind::Warning,
                );
                continue;
            }

            let cleaned = clean_cluster_manifest(raw_doc);
            self.open_resource(&canonical_kind, Some(&cleaned));
            if single_doc && let Some(tab) = self.tabs.last_mut() {
                tab.source_path = Some(file_path.to_string());
            }
            opened += 1;
        }

        if opened == 0 {
            self.notify_kind(
                format!("No supported Kubernetes resource found in '{file_path}'"),
                ToastKind::Warning,
            );
        }
        opened
    }

    /// Construct a headless/testing App without background threads.
    pub fn new_test() -> Self {
        let themes = Theme::presets().to_vec();
        let resources = yamlly_core::get_all_resources().to_vec();
        let focused_resource_kind = resources.first().map(|r| r.kind.clone());

        let mut mock = HashMap::new();
        mock.insert(
            "Deployment".to_string(),
            vec![
                serde_json::json!({
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "metadata": {
                        "name": "web-frontend",
                        "namespace": "default",
                        "uid": "1234-abcd-5678",
                        "resourceVersion": "99999",
                        "generation": 3,
                        "creationTimestamp": "2026-01-01T00:00:00Z",
                        "managedFields": [{"manager": "kubectl"}],
                        "labels": {
                            "app": "web-frontend",
                            "tier": "frontend"
                        },
                        "annotations": {
                            "kubernetes.io/change-cause": "rollout-v2",
                            "kubectl.kubernetes.io/last-applied-configuration": "{\"kind\":\"Deployment\"}"
                        }
                    },
                    "status": {
                        "availableReplicas": 3,
                        "readyReplicas": 3
                    },
                    "spec": {
                        "replicas": 3,
                        "template": {
                            "metadata": {
                                "labels": {
                                    "app": "web-frontend",
                                    "pod-template": "v2"
                                }
                            }
                        }
                    }
                }),
                serde_json::json!({
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "metadata": {
                        "name": "payments-api",
                        "namespace": "default",
                        "labels": {
                            "app": "payments-api",
                            "tier": "backend"
                        }
                    },
                    "spec": {
                        "replicas": 2,
                        "template": {
                            "metadata": {
                                "labels": {
                                    "app": "payments-api"
                                }
                            }
                        }
                    }
                }),
                serde_json::json!({
                    "apiVersion": "apps/v1",
                    "kind": "Deployment",
                    "metadata": {
                        "name": "prod-api",
                        "namespace": "production",
                        "labels": {
                            "app": "prod-api",
                            "env": "production"
                        }
                    },
                    "spec": {
                        "replicas": 5,
                        "template": {
                            "metadata": {
                                "labels": {
                                    "app": "prod-api"
                                }
                            }
                        }
                    }
                }),
            ],
        );
        mock.insert(
            "Pod".to_string(),
            vec![serde_json::json!({
                "apiVersion": "v1",
                "kind": "Pod",
                "metadata": {
                    "name": "redis-cache-0",
                    "namespace": "default",
                    "labels": {
                        "app": "redis",
                        "role": "cache"
                    }
                }
            })],
        );
        mock.insert(
            "Service".to_string(),
            vec![serde_json::json!({
                "apiVersion": "v1",
                "kind": "Service",
                "metadata": {
                    "name": "web-svc",
                    "namespace": "default",
                    "labels": {
                        "app": "web-frontend"
                    }
                }
            })],
        );
        mock.insert(
            "Namespace".to_string(),
            vec![
                serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": {
                        "name": "default"
                    }
                }),
                serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": {
                        "name": "kube-system"
                    }
                }),
                serde_json::json!({
                    "apiVersion": "v1",
                    "kind": "Namespace",
                    "metadata": {
                        "name": "production"
                    }
                }),
            ],
        );

        Self {
            running: true,
            focus: FocusState::intent(["sidebar", "resources"]),
            modals: ModalState::default(),
            toasts: ToasterState::new(),
            start_instant: Instant::now(),
            clock: Duration::ZERO,
            last_toast: None,
            theme_idx: 0,
            themes,
            resources,
            resource_filter: InputState::default(),
            selected_resource_idx: 0,
            focused_resource_kind,
            cluster_status: ClusterStatus {
                is_checking: false,
                connected: true,
                context_name: Some("test-cluster".to_string()),
                namespace: "default".to_string(),
                namespaces: vec![
                    "default".to_string(),
                    "kube-system".to_string(),
                    "production".to_string(),
                ],
                error: None,
            },
            cluster_namespaces: vec![
                "default".to_string(),
                "kube-system".to_string(),
                "production".to_string(),
            ],
            cluster_import_namespace: "default".to_string(),
            cluster_rx: None,
            cluster_fetch_rx: None,
            kubectl_op_rx: None,
            last_kubectl_op: None,
            mock_cluster_resources: Some(mock),
            tabs: Vec::new(),
            active_tab_idx: None,
            tab_seq: 0,
            kind_seq: HashMap::new(),
            confirm_modal: ConfirmModalState::default(),
            field_picker_modal: FieldPickerModalState::default(),
            preview_modal: PreviewModalState::default(),
            dir_picker_modal: DirPickerModalState::default(),
            map_editor_modal: MapEditorModalState::default(),
            select_picker_modal: SelectPickerModalState::default(),
            cluster_import_modal: ClusterImportModalState::default(),
            context_switcher_modal: ContextSwitcherModalState::default(),
            empty_input: InputState::default(),
            footer_rect: std::cell::Cell::new((0, u16::MAX, 0)),
            ui_dirty: true,
        }
    }

    #[inline]
    pub fn theme(&self) -> &Theme {
        &self.themes[self.theme_idx]
    }

    pub fn tick_clock(&mut self) {
        self.clock = self.start_instant.elapsed();
        if self.toasts.prune_expired(self.clock) {
            self.ui_dirty = true;
        }
    }

    pub fn cycle_theme(&mut self) {
        self.theme_idx = (self.theme_idx + 1) % self.themes.len();
        let name = self.themes[self.theme_idx].name;
        self.notify_kind(format!("Theme: {name}"), ToastKind::Info);
    }

    pub fn notify(&mut self, msg: impl Into<String>) {
        self.notify_kind(msg, ToastKind::Info);
    }

    pub fn notify_kind(&mut self, msg: impl Into<String>, kind: ToastKind) {
        let text = msg.into();
        self.clock = self.start_instant.elapsed();
        self.toasts.push(
            Toast::new(text.clone())
                .with_kind(kind)
                .duration(Duration::from_secs(3)),
            self.clock,
        );
        self.last_toast = Some((text, Instant::now()));
        self.ui_dirty = true;
    }

    pub fn active_toast(&self) -> Option<&str> {
        if let Some((msg, instant)) = &self.last_toast
            && instant.elapsed().as_secs_f32() < 3.0
        {
            return Some(msg.as_str());
        }
        None
    }

    pub fn poll_background(&mut self) {
        if let Some(rx) = &self.cluster_rx
            && let Ok(status) = rx.try_recv()
        {
            let was_checking = self.cluster_status.is_checking;
            let became_connected = status.connected && !self.cluster_status.connected;
            self.cluster_status = status.clone();
            if status.connected && !status.namespaces.is_empty() {
                self.cluster_namespaces = status.namespaces.clone();
                for tab in &mut self.tabs {
                    tab.form.namespaces = status.namespaces.clone();
                    tab.form.rebuild();
                }
            }
            if self.modals.is_open(MODAL_CLUSTER_IMPORT) {
                let kind = self.cluster_import_modal.selected_kind.clone();
                self.trigger_cluster_fetch(&kind);
            }
            if status.connected && was_checking {
                if became_connected {
                    self.notify_kind(
                        format!(
                            "Connected to cluster ({} namespaces loaded)",
                            self.cluster_namespaces.len()
                        ),
                        ToastKind::Success,
                    );
                } else {
                    self.notify_kind(
                        format!(
                            "✓ Refreshed cluster ({} namespaces loaded)",
                            self.cluster_namespaces.len()
                        ),
                        ToastKind::Success,
                    );
                }
            } else if !status.connected && was_checking {
                let err_msg = status.error.as_deref().unwrap_or("connection failed");
                self.notify_kind(
                    format!("Cluster connection failed: {err_msg}"),
                    ToastKind::Warning,
                );
            }
            self.ui_dirty = true;
        }
        if let Some(rx) = &self.cluster_fetch_rx
            && let Ok(fetch_res) = rx.try_recv()
            && fetch_res.kind == self.cluster_import_modal.selected_kind
        {
            self.cluster_import_modal
                .set_items(fetch_res.items, fetch_res.error);
            self.ui_dirty = true;
        }
        let completed_kubectl = self
            .kubectl_op_rx
            .as_ref()
            .and_then(|rx| rx.try_recv().ok());
        if let Some(res) = completed_kubectl {
            self.kubectl_op_rx = None;
            self.handle_kubectl_op_result(res);
        }
    }

    /// Start a non-blocking `kubectl` manifest operation (`DryRunServer`, `Diff`, or `Apply`).
    ///
    /// In headless test mode (`mock_cluster_resources.is_some()`), pushes a deterministic
    /// synthetic result through the `kubectl_op_rx` channel so `poll_background()` processes
    /// it identically to a live worker thread.
    pub fn start_kubectl_op(&mut self, op: KubectlOpKind, yaml: String) -> bool {
        if !self.cluster_status.connected {
            self.notify_kind(
                "Cluster is disconnected — reconnect or switch context first",
                ToastKind::Warning,
            );
            return false;
        }
        let (tx, rx) = channel();
        self.kubectl_op_rx = Some(rx);
        self.ui_dirty = true;

        if self.mock_cluster_resources.is_some() {
            let kind = self
                .active_tab_idx
                .and_then(|i| self.tabs.get(i))
                .map(|t| t.kind.to_ascii_lowercase())
                .unwrap_or_else(|| "resource".to_string());
            let name = self
                .active_form()
                .and_then(|f| f.field_value("metadata.name"))
                .and_then(|v| v.as_str())
                .filter(|s| !s.trim().is_empty())
                .unwrap_or("unnamed")
                .to_string();
            let mock_res = match op {
                KubectlOpKind::DryRunServer => KubectlOpResult {
                    op,
                    ok: true,
                    no_changes: false,
                    stdout: format!("{kind}/{name} configured (server dry run)"),
                    stderr: String::new(),
                },
                KubectlOpKind::Diff => KubectlOpResult {
                    op,
                    ok: true,
                    no_changes: false,
                    stdout: format!(
                        "diff -u -N LIVE/{kind}/{name} MERGED/{kind}/{name}\n\
                         --- LIVE/{kind}/{name}\n\
                         +++ MERGED/{kind}/{name}\n\
                         @@ -1,4 +1,4 @@\n\
                          apiVersion: apps/v1\n\
                          kind: Deployment\n\
                         -  replicas: 1\n\
                         +  replicas: 3"
                    ),
                    stderr: String::new(),
                },
                KubectlOpKind::Apply => KubectlOpResult {
                    op,
                    ok: true,
                    no_changes: false,
                    stdout: format!("{kind}/{name} configured"),
                    stderr: String::new(),
                },
            };
            let _ = tx.send(mock_res);
            return true;
        }

        let context = self.cluster_status.context_name.clone();
        let namespace = resolve_manifest_namespace(&yaml, Some(&self.cluster_status.namespace));
        run_kubectl_stdin_async(op, yaml, context, namespace, tx);
        true
    }

    pub fn handle_kubectl_op_result(&mut self, res: KubectlOpResult) {
        if let Some(running) = self.preview_modal.is_running_op
            && running == res.op
        {
            self.preview_modal.is_running_op = None;
            let status_msg = if res.ok {
                if res.op == KubectlOpKind::Diff {
                    let diff_text = if res.no_changes {
                        "✓ No differences — manifest matches live cluster resource.".to_string()
                    } else if res.stdout.trim().is_empty() {
                        "✓ No differences found.".to_string()
                    } else {
                        res.stdout.clone()
                    };
                    self.preview_modal.diff_output = Some(diff_text);
                    self.preview_modal.view_mode = PreviewViewMode::Diff;
                    self.preview_modal.diff_scroll = 0;
                    if res.no_changes {
                        "No differences — manifest matches live cluster resource".to_string()
                    } else {
                        "Live cluster diff ready".to_string()
                    }
                } else if res.op == KubectlOpKind::Apply {
                    res.stdout
                        .lines()
                        .next()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Applied successfully")
                        .to_string()
                } else {
                    res.stdout
                        .lines()
                        .next()
                        .map(str::trim)
                        .filter(|s| !s.is_empty())
                        .unwrap_or("Server dry-run passed")
                        .to_string()
                }
            } else {
                if res.op == KubectlOpKind::Diff {
                    self.preview_modal.diff_output = Some(format!(
                        "✕ Failed to compute diff against live cluster resource:\n{}",
                        res.stderr
                    ));
                    self.preview_modal.view_mode = PreviewViewMode::Diff;
                    self.preview_modal.diff_scroll = 0;
                }
                res.stderr
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .unwrap_or(match res.op {
                        KubectlOpKind::Apply => "Cluster apply failed",
                        KubectlOpKind::Diff => "Cluster diff failed",
                        KubectlOpKind::DryRunServer => "Server dry-run failed",
                    })
                    .to_string()
            };
            self.preview_modal.last_op = Some(res.op);
            self.preview_modal.last_status = Some((res.ok, status_msg));
        }

        if res.ok {
            let summary = if res.no_changes {
                "✓ Manifest matches live cluster resource".to_string()
            } else {
                res.stdout
                    .lines()
                    .next()
                    .map(str::trim)
                    .filter(|s| !s.is_empty())
                    .map(|s| format!("✓ {s}"))
                    .unwrap_or_else(|| format!("✓ {} succeeded", res.op.label()))
            };
            if res.op == KubectlOpKind::Apply
                && let Some(idx) = self.active_tab_idx
                && let Some(tab) = self.tabs.get_mut(idx)
            {
                tab.form.mark_clean();
                tab.dirty = false;
            }
            let toast_kind = if res.op == KubectlOpKind::Apply {
                ToastKind::Success
            } else {
                ToastKind::Info
            };
            self.notify_kind(summary, toast_kind);
        } else {
            let err_line = res
                .stderr
                .lines()
                .next()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .unwrap_or("kubectl operation failed")
                .to_string();
            self.notify_kind(
                format!("✕ {}: {err_line}", res.op.label()),
                ToastKind::Error,
            );
        }
        self.last_kubectl_op = Some(res);
        self.ui_dirty = true;
    }

    pub fn reconnect(&mut self) {
        if let Some(mock) = &self.mock_cluster_resources {
            self.cluster_status.is_checking = false;
            self.cluster_status.connected = true;
            if let Some(ns_items) = mock.get("Namespace") {
                let mut ns_list: Vec<String> = ns_items
                    .iter()
                    .filter_map(|item| item.pointer("/metadata/name").and_then(|v| v.as_str()))
                    .map(|s| s.to_string())
                    .collect();
                if !ns_list.is_empty() {
                    ns_list.sort();
                    self.cluster_namespaces = ns_list.clone();
                    for tab in &mut self.tabs {
                        tab.form.namespaces = ns_list.clone();
                        tab.form.rebuild();
                    }
                }
            }
            if self.modals.is_open(MODAL_CLUSTER_IMPORT) {
                let kind = self.cluster_import_modal.selected_kind.clone();
                self.trigger_cluster_fetch(&kind);
            }
            self.notify_kind(
                format!(
                    "✓ Refreshed cluster ({} namespaces loaded)",
                    self.cluster_namespaces.len()
                ),
                ToastKind::Success,
            );
            self.ui_dirty = true;
            return;
        }
        let (tx, rx) = channel();
        check_connection_async(tx);
        self.cluster_rx = Some(rx);
        self.cluster_status.is_checking = true;
        self.notify_kind(
            if self.cluster_status.connected {
                "Refreshing cluster resources & namespaces…"
            } else {
                "Reconnecting to Kubernetes cluster…"
            },
            ToastKind::Loading,
        );
        self.ui_dirty = true;
    }

    pub fn filtered_resources(&self) -> Vec<&ResourceIndexEntry> {
        let q = self.resource_filter.value().trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.resources.iter().collect();
        }
        self.resources
            .iter()
            .filter(|r| {
                r.kind.to_ascii_lowercase().contains(&q)
                    || r.api_version.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn active_resource_kind(&self) -> Option<String> {
        self.active_tab_idx
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.kind.clone())
    }

    pub fn active_tab(&self) -> Option<&TabInfo> {
        let idx = self.active_tab_idx?;
        self.tabs.get(idx)
    }

    pub fn active_tab_id(&self) -> Option<String> {
        self.active_tab_idx
            .and_then(|i| self.tabs.get(i))
            .map(|t| t.id.clone())
    }

    pub fn active_form(&self) -> Option<&FormState> {
        let idx = self.active_tab_idx?;
        self.tabs.get(idx).map(|t| &t.form)
    }

    pub fn active_form_mut(&mut self) -> Option<&mut FormState> {
        let idx = self.active_tab_idx?;
        self.tabs.get_mut(idx).map(|t| &mut t.form)
    }

    pub fn select_next_resource(&mut self) {
        if self.resources.is_empty() {
            return;
        }
        self.selected_resource_idx = (self.selected_resource_idx + 1) % self.resources.len();
        self.focused_resource_kind = self
            .resources
            .get(self.selected_resource_idx)
            .map(|r| r.kind.clone());
        self.ui_dirty = true;
    }

    pub fn select_prev_resource(&mut self) {
        if self.resources.is_empty() {
            return;
        }
        if self.selected_resource_idx > 0 {
            self.selected_resource_idx -= 1;
        } else {
            self.selected_resource_idx = self.resources.len() - 1;
        }
        self.focused_resource_kind = self
            .resources
            .get(self.selected_resource_idx)
            .map(|r| r.kind.clone());
        self.ui_dirty = true;
    }

    pub fn open_selected_resource(&mut self) {
        if let Some(entry) = self.resources.get(self.selected_resource_idx) {
            let kind = entry.kind.clone();
            self.open_resource(&kind, None);
        }
    }

    pub fn open_resource(&mut self, kind: &str, initial_data: Option<&serde_json::Value>) {
        self.tab_seq += 1;
        let count = self.kind_seq.entry(kind.to_string()).or_insert(0);
        *count += 1;
        let default_label = format!("{} {}", kind, *count);

        let mut dedup_ns = self.cluster_namespaces.clone();
        if !self.cluster_status.namespace.is_empty()
            && !dedup_ns.contains(&self.cluster_status.namespace)
        {
            dedup_ns.insert(0, self.cluster_status.namespace.clone());
        }
        if let Some(init) = initial_data
            && let Some(ns) = init
                .pointer("/metadata/namespace")
                .and_then(|v| v.as_str())
                .map(str::trim)
                .filter(|s| !s.is_empty())
        {
            if !self
                .cluster_namespaces
                .iter()
                .any(|existing| existing == ns)
            {
                self.cluster_namespaces.push(ns.to_string());
                self.cluster_namespaces.sort();
            }
            if !dedup_ns.iter().any(|existing| existing == ns) {
                dedup_ns.insert(0, ns.to_string());
            }
        }
        if dedup_ns.is_empty() {
            dedup_ns = vec![
                "default".to_string(),
                "kube-system".to_string(),
                "kube-public".to_string(),
            ];
        }

        let mut form = FormState::new(kind, initial_data, dedup_ns);
        // Put initial form focus on the first editable field row.
        if let Some(first_field_idx) = form
            .rows
            .iter()
            .position(|r| matches!(r.kind, Row::Field { .. }))
        {
            form.focus = first_field_idx;
        }

        let label = match yamlly_core::get_nested(&form.data, &["metadata", "name"])
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty())
        {
            Some(n) => format!("{}: {}", kind, n),
            None => default_label.clone(),
        };

        let first_field_focus_id = form
            .rows
            .iter()
            .position(|r| matches!(r.kind, Row::Field { .. }))
            .map(|idx| format!("field-{idx}"));

        let tab = TabInfo {
            id: format!("editor-tab-{}", self.tab_seq),
            label,
            default_label,
            kind: kind.to_string(),
            dirty: false,
            source_path: None,
            form,
        };

        self.tabs.push(tab);
        self.active_tab_idx = Some(self.tabs.len() - 1);
        if let Some(pos) = self.resources.iter().position(|r| r.kind == kind) {
            self.selected_resource_idx = pos;
            self.focused_resource_kind = Some(kind.to_string());
        }
        if let Some(fid) = first_field_focus_id {
            self.focus = FocusState::intent([
                ChildId::Static("workspace"),
                ChildId::Static("form"),
                ChildId::from(fid),
            ]);
        } else {
            self.focus = FocusState::intent(["workspace", "form"]);
        }
        self.ui_dirty = true;
    }

    pub fn refresh_tab_state(&mut self) {
        let Some(idx) = self.active_tab_idx else {
            return;
        };
        let Some(tab) = self.tabs.get_mut(idx) else {
            return;
        };
        let dirty = tab.form.is_dirty();
        let name = yamlly_core::get_nested(&tab.form.data, &["metadata", "name"])
            .and_then(|v| v.as_str())
            .filter(|n| !n.is_empty());
        tab.label = match name {
            Some(n) => format!("{}: {}", tab.kind, n),
            None => tab.default_label.clone(),
        };
        tab.dirty = dirty;
        self.ui_dirty = true;
    }

    pub fn validate_form(&mut self) {
        if self.active_tab_idx.is_none() {
            self.notify_kind("Open a resource first", ToastKind::Warning);
            return;
        }
        let (n, first_err_row) = self
            .active_form_mut()
            .map(|f| {
                let n = f.validate();
                let first_err = f.rows.iter().position(|r| r.error.is_some());
                (n, first_err)
            })
            .unwrap_or((0, None));
        if n == 0 {
            self.notify_kind("All fields are valid!", ToastKind::Success);
        } else {
            self.notify_kind(format!("Validation failed ({n} errors)"), ToastKind::Error);
            if let Some(err_idx) = first_err_row {
                if let Some(f) = self.active_form_mut() {
                    f.focus = err_idx;
                    f.ensure_visible(15);
                }
                self.focus = FocusState::intent([
                    ChildId::Static("workspace"),
                    ChildId::Static("form"),
                    ChildId::from(format!("field-{err_idx}")),
                ]);
            }
        }
        self.ui_dirty = true;
    }

    pub fn confirm_quit_or_prompt(&mut self) {
        let dirty: Vec<String> = self
            .tabs
            .iter()
            .filter(|t| t.dirty)
            .map(|t| t.label.clone())
            .collect();
        if dirty.is_empty() {
            self.running = false;
            return;
        }
        let message = format!(
            "{} tab(s) have unsaved changes: {}\nDiscard changes and quit?",
            dirty.len(),
            dirty.iter().take(3).cloned().collect::<Vec<_>>().join(", ")
        );
        self.confirm_modal = ConfirmModalState {
            title: "Unsaved Changes".to_string(),
            message,
            confirm_label: "Quit Without Saving".to_string(),
            kind: ConfirmKind::Quit,
        };
        let _ = self.modals.open(MODAL_CONFIRM, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn request_quit(&mut self) {
        self.confirm_quit_or_prompt();
    }

    pub fn prompt_close_tab(&mut self, id: &str) {
        let Some(tab) = self.tabs.iter().find(|t| t.id == id) else {
            return;
        };
        if tab.dirty {
            let label = tab.label.clone();
            self.confirm_modal = ConfirmModalState {
                title: "Close Unsaved Tab".to_string(),
                message: format!("Tab \"{label}\" has unsaved changes. Discard and close?"),
                confirm_label: "Discard & Close".to_string(),
                kind: ConfirmKind::CloseTab(id.to_string()),
            };
            let _ = self.modals.open(MODAL_CONFIRM, &mut self.focus);
        } else if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
            self.close_tab_at(idx);
        }
        self.ui_dirty = true;
    }

    pub fn close_active_tab(&mut self) {
        if let Some(id) = self.active_tab_id() {
            self.prompt_close_tab(&id);
        }
    }

    pub fn request_close(&mut self, id: &str) {
        self.prompt_close_tab(id);
    }

    pub fn close_tab_at(&mut self, idx: usize) {
        if idx >= self.tabs.len() {
            return;
        }
        let was_active = self.active_tab_idx == Some(idx);
        self.tabs.remove(idx);

        if self.tabs.is_empty() {
            self.active_tab_idx = None;
            self.focus = FocusState::intent(["sidebar", "resources"]);
        } else if was_active {
            self.active_tab_idx = Some(idx.min(self.tabs.len() - 1));
        } else if let Some(active) = self.active_tab_idx {
            let shift = usize::from(active > idx);
            self.active_tab_idx = Some(active - shift);
        }
        self.ui_dirty = true;
    }

    pub fn cycle_tab(&mut self, forward: bool) {
        if self.tabs.is_empty() {
            return;
        }
        let len = self.tabs.len();
        let cur = self.active_tab_idx.unwrap_or(0);
        self.active_tab_idx = Some(if forward {
            (cur + 1) % len
        } else {
            (cur + len - 1) % len
        });
        self.ui_dirty = true;
    }

    pub fn active_yaml(&self) -> Option<String> {
        let idx = self.active_tab_idx?;
        let tab = self.tabs.get(idx)?;
        Some(yamlly_core::emit_yaml_owned(tab.form.data.clone()))
    }

    pub fn close_modal(&mut self) {
        let _ = self.modals.close(&mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_field_picker(&mut self) {
        let Some(idx) = self.active_tab_idx else {
            self.notify_kind("Open a resource first", ToastKind::Warning);
            return;
        };
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        self.field_picker_modal =
            FieldPickerModalState::open_for(&tab.form.kind, &tab.form.shown_paths);
        let _ = self.modals.open(MODAL_FIELD_PICKER, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_preview(&mut self) {
        let Some(idx) = self.active_tab_idx else {
            self.notify_kind("Open a resource first", ToastKind::Warning);
            return;
        };
        let Some(tab) = self.tabs.get(idx) else {
            return;
        };
        let yaml = yamlly_core::emit_yaml_owned(tab.form.data.clone());
        let filename = match &tab.source_path {
            Some(p) if !p.trim().is_empty() => p.clone(),
            _ => {
                let name = tab
                    .form
                    .field_value("metadata.name")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.is_empty())
                    .unwrap_or("app");
                format!("{}-{}.yaml", name, tab.kind.to_lowercase())
            }
        };
        self.preview_modal = PreviewModalState::open_for(yaml, filename);
        let _ = self.modals.open(MODAL_PREVIEW, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_map_editor(&mut self, path: &str) {
        let Some(form) = self.active_form() else {
            return;
        };
        let entries = form.get_map_entries(path);
        let title = format!("Edit Map — {path}");
        self.map_editor_modal = MapEditorModalState::open_for(path, title, entries);
        let _ = self.modals.open(MODAL_MAP_EDITOR, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_select_picker(&mut self, path: &str) {
        let Some(form) = self.active_form() else {
            return;
        };
        let Some(field) = form.rows.iter().find_map(|r| match &r.kind {
            Row::Field { field, .. } if field.path == path => Some(field),
            _ => None,
        }) else {
            return;
        };
        let options = form.options_for(field);
        if options.is_empty() {
            return;
        }
        let current = form
            .field_value(path)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let title = format!("Select — {}", field.label);
        self.select_picker_modal = SelectPickerModalState::open_for(path, title, options, current);
        let _ = self.modals.open(MODAL_SELECT_PICKER, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_cluster_import_for_map(&mut self, path: &str, into_map_modal: bool) {
        if !self.cluster_status.connected {
            self.notify_kind(
                "Cluster is disconnected — reconnect or switch context first",
                ToastKind::Warning,
            );
            return;
        }
        self.cluster_import_modal = ClusterImportModalState::open_for_map(path, into_map_modal);
        let kind = self.cluster_import_modal.selected_kind.clone();
        self.trigger_cluster_fetch(&kind);
        let _ = self.modals.open(MODAL_CLUSTER_IMPORT, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_cluster_import_manifest(&mut self) {
        if !self.cluster_status.connected {
            self.notify_kind(
                "Cluster is disconnected — reconnect or switch context first",
                ToastKind::Warning,
            );
            return;
        }
        let default_kind = self
            .active_resource_kind()
            .or_else(|| self.focused_resource_kind.clone())
            .unwrap_or_else(|| "Deployment".to_string());
        self.cluster_import_modal = ClusterImportModalState::open_for_manifest(&default_kind);
        let kind = self.cluster_import_modal.selected_kind.clone();
        self.trigger_cluster_fetch(&kind);
        let _ = self.modals.open(MODAL_CLUSTER_IMPORT, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn open_context_switcher(&mut self) {
        let mut contexts = crate::k8s::get_all_contexts();
        let cur = self
            .cluster_status
            .context_name
            .clone()
            .or_else(crate::k8s::get_current_context);
        if let Some(c) = &cur
            && !contexts.contains(c)
        {
            contexts.push(c.clone());
        }
        self.context_switcher_modal = ContextSwitcherModalState::open_for(cur, contexts);
        let _ = self.modals.open(MODAL_CONTEXT_SWITCHER, &mut self.focus);
        self.ui_dirty = true;
    }

    pub fn switch_kube_context(&mut self, context_name: &str) {
        let _ = self.modals.close(&mut self.focus);
        let res = if self.mock_cluster_resources.is_some() {
            Ok(())
        } else {
            crate::k8s::use_context(context_name)
        };
        match res {
            Ok(()) => {
                self.cluster_status.context_name = Some(context_name.to_string());
                self.cluster_status.is_checking = true;
                self.cluster_status.connected = false;
                self.reconnect();
                let ns_list = if let Some(mock) = &self.mock_cluster_resources {
                    if let Some(ns_items) = mock.get("Namespace") {
                        let mut list: Vec<String> = ns_items
                            .iter()
                            .filter_map(|item| {
                                item.pointer("/metadata/name").and_then(|v| v.as_str())
                            })
                            .map(|s| s.to_string())
                            .collect();
                        list.sort();
                        list
                    } else {
                        vec!["default".to_string(), "kube-system".to_string()]
                    }
                } else {
                    crate::k8s::get_all_namespaces()
                };
                self.cluster_namespaces = ns_list.clone();
                for tab in &mut self.tabs {
                    tab.form.namespaces = ns_list.clone();
                    tab.form.rebuild();
                }
                self.notify_kind(
                    format!("Switched to Kubernetes context: {context_name}"),
                    ToastKind::Success,
                );
            }
            Err(err) => {
                self.notify_kind(format!("Failed to switch context: {err}"), ToastKind::Error);
            }
        }
        self.ui_dirty = true;
    }

    pub fn trigger_cluster_fetch(&mut self, kind: &str) {
        self.cluster_import_modal.selected_kind = kind.to_string();
        self.cluster_import_modal.is_fetching = true;

        let is_cluster_scoped = kind.eq_ignore_ascii_case("namespace")
            || kind.eq_ignore_ascii_case("node")
            || kind.eq_ignore_ascii_case("clusterrole")
            || kind.eq_ignore_ascii_case("clusterrolebinding")
            || kind.eq_ignore_ascii_case("customresourcedefinition")
            || kind.eq_ignore_ascii_case("persistentvolume")
            || kind.eq_ignore_ascii_case("storageclass");

        if is_cluster_scoped {
            self.cluster_import_modal.status_message = format!("Fetching {kind}s from cluster...");
        } else {
            self.cluster_import_modal.status_message = format!(
                "Fetching {kind}s in '{}' from cluster...",
                self.cluster_import_namespace
            );
        }
        self.cluster_import_modal.items.clear();
        self.cluster_import_modal.cursor = None;

        if let Some(mock) = &self.mock_cluster_resources {
            let mut items = mock.get(kind).cloned().unwrap_or_default();
            if !is_cluster_scoped {
                items.retain(|item| {
                    item.pointer("/metadata/namespace")
                        .and_then(|v| v.as_str())
                        .map(|ns| ns == self.cluster_import_namespace)
                        .unwrap_or(false)
                });
            }
            self.cluster_import_modal.set_items(items, None);
            return;
        }

        let ns = if is_cluster_scoped {
            None
        } else {
            Some(self.cluster_import_namespace.clone())
        };

        let (tx, rx) = channel();
        fetch_cluster_resources_async(kind.to_string(), ns, tx);
        self.cluster_fetch_rx = Some(rx);
    }

    pub fn commit_cluster_import_index(&mut self, idx_str: &str) {
        let raw_idx = idx_str.split(':').next().unwrap_or(idx_str);
        let Ok(idx) = raw_idx.parse::<usize>() else {
            return;
        };
        let Some(item) = self.cluster_import_modal.items.get(idx).cloned() else {
            return;
        };
        let item_name = item
            .pointer("/metadata/name")
            .and_then(|v| v.as_str())
            .unwrap_or("resource")
            .to_string();
        let item_ns = item
            .pointer("/metadata/namespace")
            .and_then(|v| v.as_str())
            .unwrap_or("default")
            .to_string();

        let target_path = self.cluster_import_modal.target_path.clone();
        let into_map_modal = self.cluster_import_modal.into_map_modal;
        let selected_kind = self.cluster_import_modal.selected_kind.clone();

        let _ = self.modals.close(&mut self.focus);

        match target_path {
            Some(path) => {
                let imported = extract_map_from_cluster_item(&item, &path);
                let count = imported.len();
                if into_map_modal {
                    self.map_editor_modal.merge_entries(imported);
                } else if let Some(form) = self.active_form_mut() {
                    let mut existing = form.get_map_entries(&path);
                    for (k, v) in imported {
                        if let Some(pair) = existing.iter_mut().find(|(ek, _)| ek == &k) {
                            pair.1 = v;
                        } else {
                            existing.push((k, v));
                        }
                    }
                    form.set_map(&path, existing);
                    self.refresh_tab_state();
                }
                self.notify_kind(
                    format!("Imported {count} entries from {item_ns}/{item_name}"),
                    ToastKind::Success,
                );
            }
            None => {
                let cleaned = clean_cluster_manifest(&item);
                let kind = cleaned
                    .get("kind")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&selected_kind)
                    .to_string();
                self.open_resource(&kind, Some(&cleaned));
                self.notify_kind(
                    format!("Imported {kind} {item_ns}/{item_name} from cluster"),
                    ToastKind::Success,
                );
            }
        }
    }

    /// Central Elm-style update function handling all [`Msg`] variants from `ratcn`.
    pub fn update(&mut self, msg: Msg) {
        self.ui_dirty = true;
        match msg {
            Msg::FocusChanged(focus) => {
                // Synchronize form.focus when a form field control receives ratcn focus
                if let Some(last) = focus.path().last()
                    && let Some(idx_str) = last.as_str().strip_prefix("field-")
                    && let Ok(row_idx) = idx_str.parse::<usize>()
                    && let Some(form) = self.active_form_mut()
                {
                    form.focus_row(row_idx);
                }
                self.focus = focus;
            }
            Msg::CycleTheme => self.cycle_theme(),
            Msg::Reconnect => self.reconnect(),
            Msg::ResourceFilterChanged(state) => {
                self.resource_filter = state;
                let filtered: Vec<String> = self
                    .filtered_resources()
                    .into_iter()
                    .map(|r| r.kind.clone())
                    .collect();
                if let Some(cur) = &self.focused_resource_kind {
                    if !filtered.contains(cur) {
                        self.focused_resource_kind = filtered.first().cloned();
                    }
                } else {
                    self.focused_resource_kind = filtered.first().cloned();
                }
            }
            Msg::ResourceFilterSubmit => {
                if let Some(kind) = self.focused_resource_kind.clone() {
                    self.open_resource(&kind, None);
                } else if let Some(first) =
                    self.filtered_resources().first().map(|r| r.kind.clone())
                {
                    self.open_resource(&first, None);
                }
            }
            Msg::ResourceFocusChanged(kind, _scroll) => {
                if let Some(pos) = self.resources.iter().position(|r| r.kind == kind) {
                    self.selected_resource_idx = pos;
                }
                self.focused_resource_kind = Some(kind);
            }
            Msg::ResourceSelected(kind) => {
                if let Some(pos) = self.resources.iter().position(|r| r.kind == kind) {
                    self.selected_resource_idx = pos;
                }
                self.focused_resource_kind = Some(kind.clone());
                self.open_resource(&kind, None);
            }
            Msg::TabSelected(tab_id) => {
                if let Some(pos) = self.tabs.iter().position(|t| t.id == tab_id) {
                    self.active_tab_idx = Some(pos);
                }
            }
            Msg::CloseTabById(tab_id) => {
                self.prompt_close_tab(&tab_id);
            }
            Msg::CloseActiveTab => {
                self.close_active_tab();
            }
            Msg::CycleTab(forward) => {
                self.cycle_tab(forward);
            }
            Msg::ValidateForm => {
                self.validate_form();
            }
            Msg::OpenPreview => {
                self.open_preview();
            }
            Msg::OpenFieldPicker => {
                self.open_field_picker();
            }
            Msg::OpenMapEditor(path) => {
                self.open_map_editor(&path);
            }
            Msg::OpenSelectPicker(path) => {
                self.open_select_picker(&path);
            }
            Msg::OpenClusterImport(path) => {
                self.open_cluster_import_for_map(&path, false);
            }
            Msg::OpenClusterImportFromMapEditor => {
                let path = self.map_editor_modal.path.clone();
                self.open_cluster_import_for_map(&path, true);
            }
            Msg::OpenClusterImportManifest => {
                self.open_cluster_import_manifest();
            }
            Msg::AddArrayItemAt(array_path) => {
                if let Some(form) = self.active_form_mut() {
                    form.add_items_at(&array_path, 1);
                    let focus_idx = form.focus;
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{focus_idx}")),
                    ]);
                }
                self.refresh_tab_state();
                self.notify_kind(format!("Added item to {array_path}"), ToastKind::Success);
            }
            Msg::RemoveArrayItemAt(array_path) => {
                let mut removed = false;
                if let Some(form) = self.active_form_mut()
                    && form.remove_array_item_at(&array_path)
                {
                    removed = true;
                    let focus_idx = form.focus;
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{focus_idx}")),
                    ]);
                }
                if removed {
                    self.refresh_tab_state();
                    self.notify_kind(format!("Removed item from {array_path}"), ToastKind::Info);
                }
            }
            Msg::RemoveField(field_path) => {
                let mut removed_label = None;
                if let Some(form) = self.active_form_mut()
                    && let Some(label) = form.remove_field(&field_path)
                {
                    let focus_idx = form.focus;
                    removed_label = Some((label, focus_idx));
                }
                if let Some((label, focus_idx)) = removed_label {
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{focus_idx}")),
                    ]);
                    self.refresh_tab_state();
                    self.notify_kind(format!("Removed field '{label}'"), ToastKind::Info);
                } else {
                    self.notify_kind(
                        "Default schema fields cannot be removed",
                        ToastKind::Warning,
                    );
                }
            }
            Msg::RemoveFocusedField => {
                let mut removed_label = None;
                let mut has_focused = false;
                if let Some(form) = self.active_form_mut() {
                    has_focused = form.focused_field().is_some();
                    if let Some(label) = form.remove_focused_field() {
                        let focus_idx = form.focus;
                        removed_label = Some((label, focus_idx));
                    }
                }
                if let Some((label, focus_idx)) = removed_label {
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{focus_idx}")),
                    ]);
                    self.refresh_tab_state();
                    self.notify_kind(format!("Removed field '{label}'"), ToastKind::Info);
                } else if has_focused {
                    self.notify_kind(
                        "Default schema fields cannot be removed",
                        ToastKind::Warning,
                    );
                }
            }
            Msg::RemoveCategory(row_idx) => {
                let mut removed_info = None;
                if let Some(form) = self.active_form_mut()
                    && let Some((cat_label, count)) = form.remove_category_at(row_idx)
                {
                    let focus_idx = form.focus;
                    removed_info = Some((cat_label, count, focus_idx));
                }
                if let Some((cat_label, count, focus_idx)) = removed_info {
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{focus_idx}")),
                    ]);
                    self.refresh_tab_state();
                    self.notify_kind(
                        format!(
                            "Removed '{cat_label}' ({count} field{})",
                            if count == 1 { "" } else { "s" }
                        ),
                        ToastKind::Info,
                    );
                }
            }
            Msg::ToggleFormCategory(row_idx) => {
                if let Some(form) = self.active_form_mut() {
                    form.toggle_category_at(row_idx);
                }
            }
            Msg::FormScrollChanged(offset) => {
                if let Some(form) = self.active_form_mut() {
                    form.set_scroll(offset as usize);
                }
            }
            Msg::FormInputChanged { row_idx, state } => {
                if let Some(form) = self.active_form_mut() {
                    form.set_input_state(row_idx, state);
                }
                self.refresh_tab_state();
            }
            Msg::FormInputSubmit { row_idx } => {
                if let Some(form) = self.active_form_mut() {
                    form.focus_next_field_from(row_idx, true);
                    let next_idx = form.focus;
                    self.focus = FocusState::intent([
                        ChildId::Static("workspace"),
                        ChildId::Static("form"),
                        ChildId::from(format!("field-{next_idx}")),
                    ]);
                }
            }
            Msg::FormBooleanChanged { row_idx, checked } => {
                if let Some(form) = self.active_form_mut() {
                    form.set_boolean_at(row_idx, checked);
                }
                self.refresh_tab_state();
            }
            Msg::FormOptionCycleChanged {
                row_idx,
                option_idx,
            } => {
                if let Some(form) = self.active_form_mut() {
                    form.set_option_index_at(row_idx, option_idx);
                }
                self.refresh_tab_state();
            }
            Msg::CloseModal => {
                self.close_modal();
            }
            Msg::ConfirmAccepted => {
                let kind = self.confirm_modal.kind.clone();
                let _ = self.modals.close(&mut self.focus);
                match kind {
                    ConfirmKind::Quit => self.running = false,
                    ConfirmKind::CloseTab(id) => {
                        if let Some(idx) = self.tabs.iter().position(|t| t.id == id) {
                            self.close_tab_at(idx);
                        }
                    }
                    ConfirmKind::ApplyManifest => {
                        let yaml = self.preview_modal.yaml.clone();
                        if self.start_kubectl_op(KubectlOpKind::Apply, yaml) {
                            self.preview_modal.is_running_op = Some(KubectlOpKind::Apply);
                            self.preview_modal.last_op = Some(KubectlOpKind::Apply);
                            self.preview_modal.last_status = None;
                            self.notify_kind("Applying manifest to cluster…", ToastKind::Loading);
                        }
                    }
                }
            }
            Msg::FieldPickerFilterChanged(state) => {
                self.field_picker_modal.set_filter(state);
            }
            Msg::FieldPickerCursorChanged(path, _scroll) => {
                self.field_picker_modal.cursor = Some(path);
            }
            Msg::FieldPickerToggle(path) => {
                self.field_picker_modal.toggle(path);
            }
            Msg::FieldPickerToggleGroup => {
                if let Some((group, selected)) =
                    self.field_picker_modal.toggle_focused_group_selection()
                {
                    self.notify_kind(
                        if selected {
                            format!("Selected all fields in {group}")
                        } else {
                            format!("Deselected all fields in {group}")
                        },
                        ToastKind::Info,
                    );
                    self.ui_dirty = true;
                }
            }
            Msg::FieldPickerCommit => {
                let paths = self.field_picker_modal.paths_to_add();
                let _ = self.modals.close(&mut self.focus);
                if !paths.is_empty() {
                    let count = paths.len();
                    if let Some(form) = self.active_form_mut() {
                        form.rebuild_with_extra(&paths);
                    }
                    self.refresh_tab_state();
                    self.notify_kind(
                        format!("Added {count} field{}", if count == 1 { "" } else { "s" }),
                        ToastKind::Success,
                    );
                }
            }
            Msg::PreviewScrollChanged(offset) => {
                self.preview_modal.scroll = offset;
            }
            Msg::PreviewDiffScrollChanged(offset) => {
                self.preview_modal.diff_scroll = offset;
            }
            Msg::PreviewPathChanged(state) => {
                self.preview_modal.path_input = state;
            }
            Msg::PreviewSave => {
                let Some(target_path) = self.preview_modal.resolve_save_path() else {
                    self.notify_kind("File path cannot be empty", ToastKind::Error);
                    return;
                };
                let yaml = self.preview_modal.yaml.clone();
                if let Some(parent) = target_path.parent()
                    && !parent.as_os_str().is_empty()
                    && let Err(e) = std::fs::create_dir_all(parent)
                {
                    self.notify_kind(
                        format!("Failed to create directory {}: {e}", parent.display()),
                        ToastKind::Error,
                    );
                    return;
                }
                match std::fs::write(&target_path, yaml.as_bytes()) {
                    Ok(()) => {
                        let display_path = crate::modals::shorten_home_path(&target_path);
                        self.preview_modal.path_input = InputState::new(display_path.clone());
                        let _ = self.modals.close(&mut self.focus);
                        if let Some(idx) = self.active_tab_idx
                            && let Some(tab) = self.tabs.get_mut(idx)
                        {
                            tab.dirty = false;
                            tab.source_path = Some(display_path.clone());
                            tab.form.mark_clean();
                        }
                        self.notify_kind(format!("Saved → {display_path}"), ToastKind::Success);
                    }
                    Err(e) => {
                        self.notify_kind(format!("Save failed: {e}"), ToastKind::Error);
                    }
                }
            }
            Msg::PreviewDryRun => {
                if !self.cluster_status.connected {
                    self.notify_kind(
                        "Cluster is disconnected — reconnect or switch context first",
                        ToastKind::Warning,
                    );
                    return;
                }
                let yaml = self.preview_modal.yaml.clone();
                if self.start_kubectl_op(KubectlOpKind::DryRunServer, yaml) {
                    self.preview_modal.is_running_op = Some(KubectlOpKind::DryRunServer);
                    self.preview_modal.last_status = None;
                    self.notify_kind("Running server dry-run validation…", ToastKind::Loading);
                }
            }
            Msg::PreviewDiff => {
                if !self.cluster_status.connected {
                    self.notify_kind(
                        "Cluster is disconnected — reconnect or switch context first",
                        ToastKind::Warning,
                    );
                    return;
                }
                self.preview_modal.view_mode = PreviewViewMode::Diff;
                let yaml = self.preview_modal.yaml.clone();
                if self.start_kubectl_op(KubectlOpKind::Diff, yaml) {
                    self.preview_modal.is_running_op = Some(KubectlOpKind::Diff);
                    self.preview_modal.last_op = Some(KubectlOpKind::Diff);
                    self.preview_modal.last_status = None;
                    self.notify_kind("Computing live cluster diff…", ToastKind::Loading);
                }
            }
            Msg::PreviewApply => {
                if !self.cluster_status.connected {
                    self.notify_kind(
                        "Cluster is disconnected — reconnect or switch context first",
                        ToastKind::Warning,
                    );
                    return;
                }
                let yaml = &self.preview_modal.yaml;
                let kind = self
                    .active_tab()
                    .map(|t| t.kind.clone())
                    .unwrap_or_else(|| "Resource".to_string());
                let name = self
                    .active_tab()
                    .and_then(|t| t.form.field_value("metadata.name"))
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .unwrap_or("unnamed")
                    .to_string();
                let context = self
                    .cluster_status
                    .context_name
                    .clone()
                    .unwrap_or_else(|| "current-context".to_string());
                let effective_ns =
                    resolve_manifest_namespace(yaml, Some(&self.cluster_status.namespace))
                        .unwrap_or_else(|| "cluster-scoped".to_string());

                let message = format!(
                    "Apply {kind} '{name}' to cluster context '{context}' (namespace: '{effective_ns}')?\n\nThis will configure or update live cluster resources.",
                );

                self.confirm_modal = ConfirmModalState {
                    title: "Apply Manifest to Cluster".to_string(),
                    message,
                    confirm_label: "☸ Apply to Cluster".to_string(),
                    kind: ConfirmKind::ApplyManifest,
                };
                let _ = self.modals.open(MODAL_CONFIRM, &mut self.focus);
                self.ui_dirty = true;
            }
            Msg::PreviewToggleView(target) => {
                let next_mode = target.unwrap_or(match self.preview_modal.view_mode {
                    PreviewViewMode::Yaml => PreviewViewMode::Diff,
                    PreviewViewMode::Diff => PreviewViewMode::Yaml,
                });
                self.preview_modal.view_mode = next_mode;
                if next_mode == PreviewViewMode::Diff && self.preview_modal.diff_output.is_none() {
                    self.update(Msg::PreviewDiff);
                }
            }
            Msg::OpenDirPicker => {
                let raw = self.preview_modal.path_input.value().trim();
                let start = if raw.is_empty() {
                    crate::modals::default_save_dir()
                } else {
                    crate::modals::resolve_abs_dir(Path::new(raw))
                };
                self.dir_picker_modal = DirPickerModalState::open_at(start);
                let _ = self.modals.open(MODAL_DIR_PICKER, &mut self.focus);
            }
            Msg::DirPickerCursorChanged(name, _scroll) => {
                self.dir_picker_modal.cursor = Some(name);
            }
            Msg::DirPickerEnterDir(name) => {
                if self.dir_picker_modal.enter_named(&name) {
                    let chosen = self.dir_picker_modal.current_dir.clone();
                    self.preview_modal.set_directory(&chosen);
                    let _ = self.modals.close(&mut self.focus);
                }
            }
            Msg::DirPickerUp => {
                self.dir_picker_modal.go_up();
            }
            Msg::DirPickerSelectCurrent => {
                let chosen = self.dir_picker_modal.selected_dir();
                self.preview_modal.set_directory(&chosen);
                let _ = self.modals.close(&mut self.focus);
            }
            Msg::MapEditorScrollChanged(offset) => {
                self.map_editor_modal.scroll = offset;
            }
            Msg::MapEditorKeyChanged { row_idx, state } => {
                if let Some(row) = self.map_editor_modal.rows.get_mut(row_idx) {
                    row.key = state;
                }
            }
            Msg::MapEditorValueChanged { row_idx, state } => {
                if let Some(row) = self.map_editor_modal.rows.get_mut(row_idx) {
                    row.value = state;
                }
            }
            Msg::MapEditorAddRow => {
                self.map_editor_modal.add_row(String::new(), String::new());
                if let Some(last) = self.map_editor_modal.rows.last() {
                    let row_id = last.id.clone();
                    self.focus = FocusState::intent([
                        ChildId::Static(MODAL_MAP_EDITOR),
                        ChildId::Static("scroll"),
                        row_id,
                        ChildId::Static("key"),
                    ]);
                }
            }
            Msg::MapEditorRemoveRow(idx) => {
                self.map_editor_modal.remove_row(idx);
            }
            Msg::MapEditorSave => {
                let path = self.map_editor_modal.path.clone();
                let entries = self.map_editor_modal.collect_entries();
                let _ = self.modals.close(&mut self.focus);
                if let Some(form) = self.active_form_mut() {
                    form.set_map(&path, entries);
                }
                self.refresh_tab_state();
                self.notify_kind(format!("Updated {path}"), ToastKind::Success);
            }
            Msg::SelectPickerCursorChanged(opt, _scroll) => {
                self.select_picker_modal.cursor = Some(opt);
            }
            Msg::SelectPickerCommit(opt) => {
                let path = self.select_picker_modal.path.clone();
                let _ = self.modals.close(&mut self.focus);
                let val = if opt == SELECT_UNSET_VALUE {
                    None
                } else {
                    Some(opt)
                };
                if let Some(form) = self.active_form_mut() {
                    form.set_select_value(&path, val);
                }
                self.refresh_tab_state();
            }
            Msg::SelectPickerClear => {
                let path = self.select_picker_modal.path.clone();
                let _ = self.modals.close(&mut self.focus);
                if let Some(form) = self.active_form_mut() {
                    form.set_select_value(&path, None);
                }
                self.refresh_tab_state();
            }
            Msg::ClusterImportKindSelected(kind) => {
                if kind != self.cluster_import_modal.selected_kind {
                    self.trigger_cluster_fetch(&kind);
                }
            }
            Msg::ClusterImportRefresh => {
                self.reconnect();
                let kind = self.cluster_import_modal.selected_kind.clone();
                self.trigger_cluster_fetch(&kind);
            }
            Msg::ClusterImportNamespaceOpen(open) => {
                self.cluster_import_modal.namespace_open = open;
                self.ui_dirty = true;
            }
            Msg::ClusterImportNamespaceSelected(ns) => {
                if self.cluster_import_namespace != ns {
                    self.cluster_import_namespace = ns;
                    let kind = self.cluster_import_modal.selected_kind.clone();
                    self.trigger_cluster_fetch(&kind);
                }
                self.cluster_import_modal.namespace_open = false;
                self.ui_dirty = true;
            }
            Msg::ClusterImportNamespaceCursorChanged(ns) => {
                self.cluster_import_modal.namespace_cursor = Some(ns);
                self.ui_dirty = true;
            }
            Msg::ClusterImportFilterChanged(state) => {
                self.cluster_import_modal.filter = state;
                let filtered = self.cluster_import_modal.filtered_indices();
                if let Some(cur_idx) = self.cluster_import_modal.selected_index() {
                    self.cluster_import_modal.cursor = Some(cur_idx.to_string());
                } else {
                    self.cluster_import_modal.cursor = filtered.first().map(|i| i.to_string());
                }
                self.ui_dirty = true;
            }
            Msg::ClusterImportCursorChanged(idx_str, _scroll) => {
                self.cluster_import_modal.cursor = Some(idx_str);
            }
            Msg::ClusterImportCommit(idx_str) => {
                self.commit_cluster_import_index(&idx_str);
            }
            Msg::ClusterImportCommitSelected => {
                if let Some(idx) = self.cluster_import_modal.selected_index() {
                    self.commit_cluster_import_index(&idx.to_string());
                } else if let Some(cur) = self.cluster_import_modal.cursor.clone() {
                    self.commit_cluster_import_index(&cur);
                }
            }
            Msg::OpenContextSwitcher => {
                self.open_context_switcher();
            }
            Msg::ContextSwitcherFilterChanged(state) => {
                self.context_switcher_modal.filter = state;
                let filtered = self.context_switcher_modal.filtered_contexts();
                if let Some(cur) = &self.context_switcher_modal.cursor {
                    if !filtered.contains(cur) {
                        self.context_switcher_modal.cursor = filtered.first().cloned();
                    }
                } else {
                    self.context_switcher_modal.cursor = filtered.first().cloned();
                }
                self.ui_dirty = true;
            }
            Msg::ContextSwitcherCursorChanged(name, _scroll) => {
                self.context_switcher_modal.cursor = Some(name);
            }
            Msg::ContextSwitcherCommit(ctx_name) => {
                self.switch_kube_context(&ctx_name);
            }
            Msg::ContextSwitcherCommitSelected => {
                if let Some(ctx_name) = self.context_switcher_modal.selected_context() {
                    self.switch_kube_context(&ctx_name);
                }
            }
            Msg::RequestQuit => {
                self.confirm_quit_or_prompt();
            }
        }
    }

    /// Global `Ctrl+<key>` application shortcuts that take precedence over
    /// readline bindings inside a focused text `Input` when no modal is open.
    pub fn handle_global_ctrl_shortcut(&mut self, event: &Event) -> bool {
        if self.modals.top().is_some() {
            return false;
        }
        let Event::Key(key) = event else {
            return false;
        };
        if key.modifiers.ctrl && !key.modifiers.alt {
            match key.code {
                KeyCode::Char('q' | 'Q') => {
                    self.update(Msg::RequestQuit);
                    return true;
                }
                KeyCode::Char('s' | 'S') => {
                    self.update(Msg::OpenPreview);
                    return true;
                }
                KeyCode::Char('v' | 'V') => {
                    self.update(Msg::ValidateForm);
                    return true;
                }
                KeyCode::Char('f' | 'F') => {
                    self.update(Msg::OpenFieldPicker);
                    return true;
                }
                KeyCode::Char('w' | 'W') => {
                    self.update(Msg::CloseActiveTab);
                    return true;
                }
                KeyCode::Char('i' | 'I') => {
                    self.update(Msg::OpenClusterImportManifest);
                    return true;
                }
                KeyCode::Char('k' | 'K') => {
                    self.update(Msg::OpenContextSwitcher);
                    return true;
                }
                KeyCode::Char('r' | 'R') => {
                    self.update(Msg::Reconnect);
                    return true;
                }
                KeyCode::Char('d' | 'D') => {
                    self.update(Msg::RemoveFocusedField);
                    return true;
                }
                _ => {}
            }
        }
        false
    }

    /// Fallback event handler for global shortcuts and cross-pane navigation
    /// when `ratcn.handle_event` returns `EventResult::Ignored`.
    pub fn handle_fallback_event(&mut self, event: Event) -> bool {
        let Event::Key(key) = event else {
            return false;
        };

        // When a modal is open, only modal-level fallbacks apply.
        if let Some(top_modal) = self.modals.top().cloned() {
            if key.code == KeyCode::Esc && !key.modifiers.any() {
                if top_modal.as_str() == MODAL_CLUSTER_IMPORT
                    && self.cluster_import_modal.namespace_open
                {
                    self.cluster_import_modal.namespace_open = false;
                    self.ui_dirty = true;
                    return true;
                }
                self.close_modal();
                return true;
            }
            match top_modal.as_str() {
                MODAL_FIELD_PICKER => {
                    if !key.modifiers.any() {
                        match key.code {
                            KeyCode::Enter => {
                                self.update(Msg::FieldPickerCommit);
                                return true;
                            }
                            KeyCode::Left => {
                                if self.field_picker_modal.expand_or_collapse_cursor(false) {
                                    self.ui_dirty = true;
                                }
                                return true;
                            }
                            KeyCode::Right => {
                                if self.field_picker_modal.expand_or_collapse_cursor(true) {
                                    self.ui_dirty = true;
                                }
                                return true;
                            }
                            KeyCode::Char('a') | KeyCode::Char('A') => {
                                self.update(Msg::FieldPickerToggleGroup);
                                return true;
                            }
                            _ => return false,
                        }
                    }
                    return false;
                }
                MODAL_CLUSTER_IMPORT => {
                    if !key.modifiers.any() {
                        match key.code {
                            KeyCode::Enter => {
                                self.update(Msg::ClusterImportCommitSelected);
                                return true;
                            }
                            KeyCode::Up => {
                                self.cluster_import_modal.select_next(false);
                                self.ui_dirty = true;
                                return true;
                            }
                            KeyCode::Down => {
                                self.cluster_import_modal.select_next(true);
                                self.ui_dirty = true;
                                return true;
                            }
                            _ => return false,
                        }
                    }
                    return false;
                }
                MODAL_CONTEXT_SWITCHER => {
                    if !key.modifiers.any() {
                        match key.code {
                            KeyCode::Enter => {
                                self.update(Msg::ContextSwitcherCommitSelected);
                                return true;
                            }
                            KeyCode::Up => {
                                self.context_switcher_modal.select_next(false);
                                self.ui_dirty = true;
                                return true;
                            }
                            KeyCode::Down => {
                                self.context_switcher_modal.select_next(true);
                                self.ui_dirty = true;
                                return true;
                            }
                            _ => return false,
                        }
                    }
                    return false;
                }
                MODAL_DIR_PICKER if key.code == KeyCode::Backspace && !key.modifiers.any() => {
                    self.update(Msg::DirPickerUp);
                    return true;
                }
                MODAL_PREVIEW => {
                    if !key.modifiers.any()
                        && !self.focus.contains_path([MODAL_PREVIEW, "path-input"])
                    {
                        match key.code {
                            KeyCode::Char('d' | 'D') => {
                                self.update(Msg::PreviewDryRun);
                                return true;
                            }
                            KeyCode::Char('f' | 'F') => {
                                self.update(Msg::PreviewToggleView(Some(PreviewViewMode::Diff)));
                                return true;
                            }
                            KeyCode::Char('y' | 'Y') => {
                                self.update(Msg::PreviewToggleView(Some(PreviewViewMode::Yaml)));
                                return true;
                            }
                            KeyCode::Char('a' | 'A') => {
                                self.update(Msg::PreviewApply);
                                return true;
                            }
                            KeyCode::Tab => {
                                self.update(Msg::PreviewToggleView(None));
                                return true;
                            }
                            _ => return false,
                        }
                    }
                    return false;
                }
                _ => return false,
            }
        }

        if self.handle_global_ctrl_shortcut(&event) {
            return true;
        }

        // Up/Down/Left/Right inside the form scope
        if !key.modifiers.any() && self.focus.contains_path(["workspace", "form"]) {
            match key.code {
                KeyCode::Down => {
                    if let Some(form) = self.active_form_mut() {
                        form.focus_next_field(true);
                        let idx = form.focus;
                        self.focus = FocusState::intent([
                            ChildId::Static("workspace"),
                            ChildId::Static("form"),
                            ChildId::from(format!("field-{idx}")),
                        ]);
                        self.ui_dirty = true;
                        return true;
                    }
                }
                KeyCode::Up => {
                    if let Some(form) = self.active_form_mut() {
                        form.focus_next_field(false);
                        let idx = form.focus;
                        self.focus = FocusState::intent([
                            ChildId::Static("workspace"),
                            ChildId::Static("form"),
                            ChildId::from(format!("field-{idx}")),
                        ]);
                        self.ui_dirty = true;
                        return true;
                    }
                }
                KeyCode::Right => {
                    if let Some(form) = self.active_form_mut() {
                        form.move_cursor(true);
                        self.refresh_tab_state();
                        return true;
                    }
                }
                KeyCode::Left => {
                    if let Some(form) = self.active_form_mut() {
                        form.move_cursor(false);
                        self.refresh_tab_state();
                        return true;
                    }
                }
                _ => {}
            }
        }

        // Single-key shortcuts (only reached when the focused component did NOT consume the character, e.g. outside text inputs or on Esc)
        if !key.modifiers.ctrl && !key.modifiers.alt {
            match key.code {
                KeyCode::Esc => {
                    if !self.focus.contains_path(["sidebar", "resources"]) {
                        self.focus = FocusState::intent(["sidebar", "resources"]);
                        self.ui_dirty = true;
                        return true;
                    }
                }
                KeyCode::Char('q') => {
                    self.update(Msg::RequestQuit);
                    return true;
                }
                KeyCode::Char('p') => {
                    self.update(Msg::OpenPreview);
                    return true;
                }
                KeyCode::Char('t') => {
                    self.update(Msg::CycleTheme);
                    return true;
                }
                KeyCode::Char('i') => {
                    self.update(Msg::OpenClusterImportManifest);
                    return true;
                }
                KeyCode::Char('c') => {
                    self.update(Msg::OpenContextSwitcher);
                    return true;
                }
                KeyCode::Char('r') => {
                    self.update(Msg::Reconnect);
                    return true;
                }
                KeyCode::F(5) => {
                    self.update(Msg::Reconnect);
                    return true;
                }
                KeyCode::Char('/') => {
                    self.focus = FocusState::intent(["sidebar", "filter"]);
                    self.ui_dirty = true;
                    return true;
                }
                KeyCode::Char('+') => {
                    self.update(Msg::OpenFieldPicker);
                    return true;
                }
                KeyCode::Char('a') if self.focus.contains_path(["workspace", "form"]) => {
                    if let Some(form) = self.active_form_mut()
                        && form.add_array_item()
                    {
                        let idx = form.focus;
                        self.focus = FocusState::intent([
                            ChildId::Static("workspace"),
                            ChildId::Static("form"),
                            ChildId::from(format!("field-{idx}")),
                        ]);
                        self.refresh_tab_state();
                        self.notify_kind("Added array item", ToastKind::Success);
                        return true;
                    }
                }
                KeyCode::Char('[') => {
                    self.update(Msg::CycleTab(false));
                    return true;
                }
                KeyCode::Char(']') => {
                    self.update(Msg::CycleTab(true));
                    return true;
                }
                _ => {}
            }
        }

        false
    }

    /// Helper for tests or programmatic key dispatch without a separate `Ratcn` call.
    pub fn key_event(code: KeyCode) -> Event {
        Event::Key(KeyEvent::new(code))
    }
}
