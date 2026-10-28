//! State models for all `ratcn::Dialog` modal layers in `yamlly`.
//!
//! Each modal is opened and closed through [`ratcn::runtime::ModalState`], which
//! preserves and restores focus automatically, and is rendered via
//! [`ratcn::Dialog`] on its own modal layer.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use ratcn::InputState;
use ratcn::runtime::ChildId;
use yamlly_core::LeafFieldPath;

pub const MODAL_CONFIRM: &str = "confirm";
pub const MODAL_FIELD_PICKER: &str = "field_picker";
pub const MODAL_PREVIEW: &str = "preview";
pub const MODAL_DIR_PICKER: &str = "dir_picker";
pub const MODAL_MAP_EDITOR: &str = "map_editor";
pub const MODAL_SELECT_PICKER: &str = "select_picker";
pub const MODAL_CLUSTER_IMPORT: &str = "cluster_import";
pub const MODAL_CONTEXT_SWITCHER: &str = "context_switcher";

/// Special dropdown option value that clears an Enum / Namespace field.
pub const SELECT_UNSET_VALUE: &str = "__unset__";

/// What a confirmation dialog is guarding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmKind {
    /// Quit the application while one or more tabs have unsaved changes.
    Quit,
    /// Close a specific tab that has unsaved changes.
    CloseTab(String),
}

/// State for the confirmation [`ratcn::Dialog`].
#[derive(Debug, Clone)]
pub struct ConfirmModalState {
    pub title: String,
    pub message: String,
    pub confirm_label: String,
    pub kind: ConfirmKind,
}

impl Default for ConfirmModalState {
    fn default() -> Self {
        Self {
            title: "Confirm".to_string(),
            message: String::new(),
            confirm_label: "Confirm".to_string(),
            kind: ConfirmKind::Quit,
        }
    }
}

/// Prefix used for category/group row IDs in the field picker tree.
pub const FIELD_PICKER_GROUP_PREFIX: &str = "group:";

/// A single visible row in the hierarchical Optional Field Selector tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldPickerRow {
    /// An expandable/collapsible category branch (`📁 metadata`, `📁 strategy`, `📦 containers[]`).
    Category {
        /// Row ID (`"group:<group_path>"`).
        id: String,
        /// Dotted path of this category (`"spec.strategy"`).
        group_path: String,
        /// Display name of this segment (`"strategy"`, `"containers[]"`).
        label: String,
        /// Indentation depth (`0, 1, 2, ...`).
        depth: usize,
        /// True if this category is currently expanded (`▾`), false if collapsed (`▸`).
        expanded: bool,
        /// True if this category represents an array (`label.ends_with("[]")`).
        is_array: bool,
        /// Number of available leaf fields nested under this category.
        total_leaves: usize,
        /// Number of currently selected leaf fields nested under this category.
        selected_leaves: usize,
    },
    /// A selectable leaf schema field (`[ ] minReadySeconds  [integer]`).
    Leaf {
        /// Full dotted schema path (`"spec.minReadySeconds"`).
        path: String,
        /// Short leaf field name (`"minReadySeconds"`).
        label: String,
        /// Schema field type (`"integer"`, `"map[string]string"`, etc.).
        field_type: String,
        /// Whether the schema marks this leaf as required.
        required: bool,
        /// Indentation depth (`1, 2, ...`).
        depth: usize,
        /// Whether this leaf is currently selected (`[✓]`).
        selected: bool,
    },
}

impl FieldPickerRow {
    pub fn id(&self) -> &str {
        match self {
            FieldPickerRow::Category { id, .. } => id.as_str(),
            FieldPickerRow::Leaf { path, .. } => path.as_str(),
        }
    }
}

/// Canonicalize a schema or data path by replacing `[0]`, `[1]`, etc. with `[]`
/// and stripping any trailing `[]` on scalar array leaves (`spec.accessModes[]` -> `spec.accessModes`).
pub fn canonical_schema_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'['
            && let Some(end) = path[i..].find(']')
        {
            out.push_str("[]");
            i += end + 1;
            continue;
        }
        let ch = path[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out.strip_suffix("[]").unwrap_or(&out).to_string()
}

fn is_envelope_prefix(path: &str) -> bool {
    matches!(
        path,
        "" | "metadata"
            | "spec"
            | "spec.template"
            | "spec.template.metadata"
            | "spec.template.spec"
            | "spec.jobTemplate"
            | "spec.jobTemplate.spec"
            | "spec.jobTemplate.spec.template"
            | "spec.jobTemplate.spec.template.spec"
    )
}

/// Returns `true` if `leaf_path` (or its singleton category/map container) has already
/// been added to the form in `shown_paths`.
pub fn is_leaf_already_added(leaf_path: &str, shown_paths: &[String]) -> bool {
    let norm_leaf = canonical_schema_path(leaf_path);
    let leaf_prefix = format!("{norm_leaf}.");
    for shown in shown_paths {
        let norm_shown = canonical_schema_path(shown);
        if norm_shown == norm_leaf {
            return true;
        }
        // If shown_paths already has sub-keys under `norm_leaf` (e.g.,
        // `...resources.requests.cpu` is in `shown_paths` while `...resources.requests`
        // is a map leaf in the schema), that singleton category is already in the form.
        if norm_shown.starts_with(&leaf_prefix) {
            return true;
        }
        // Conversely, if a parent map field is already in `shown_paths`, its sub-keys
        // are already covered by that map editor.
        let shown_prefix = format!("{norm_shown}.");
        if norm_leaf.starts_with(&shown_prefix) {
            return true;
        }
    }

    // Check if leaf belongs to a non-repeatable category (an object/map category that cannot
    // appear more than once) where any field from that category has already been added to the form.
    let parts: Vec<&str> = norm_leaf.split('.').collect();
    for i in 1..parts.len() {
        let group_path = parts[..i].join(".");
        if is_envelope_prefix(&group_path) || group_path.ends_with("[]") {
            continue;
        }
        let group_prefix = format!("{group_path}.");
        let category_already_added = shown_paths.iter().any(|s| {
            let ns = canonical_schema_path(s);
            ns == group_path || ns.starts_with(&group_prefix)
        });
        if category_already_added {
            return true;
        }
    }

    false
}

struct PickerTreeBranch<'a> {
    name: String,
    group_path: String,
    leaves: Vec<&'a LeafFieldPath>,
    children: Vec<PickerTreeBranch<'a>>,
}

impl<'a> PickerTreeBranch<'a> {
    fn new(name: String, group_path: String) -> Self {
        Self {
            name,
            group_path,
            leaves: Vec::new(),
            children: Vec::new(),
        }
    }

    fn insert(
        &mut self,
        parts: &[&str],
        full_parts: &[&str],
        depth: usize,
        leaf: &'a LeafFieldPath,
    ) {
        if parts.len() <= 1 {
            self.leaves.push(leaf);
            return;
        }
        let seg = parts[0];
        let gp = full_parts[..=depth].join(".");
        let idx = match self.children.iter().position(|c| c.name == seg) {
            Some(i) => i,
            None => {
                self.children
                    .push(PickerTreeBranch::new(seg.to_string(), gp));
                self.children.len() - 1
            }
        };
        self.children[idx].insert(&parts[1..], full_parts, depth + 1, leaf);
    }

    fn counts(&self, selected: &BTreeSet<String>) -> (usize, usize) {
        let mut total = self.leaves.len();
        let mut sel = self
            .leaves
            .iter()
            .filter(|l| selected.contains(&l.path))
            .count();
        for ch in &self.children {
            let (ct, cs) = ch.counts(selected);
            total += ct;
            sel += cs;
        }
        (total, sel)
    }
}

/// State for the schema field picker [`ratcn::Dialog`].
#[derive(Clone, Default)]
pub struct FieldPickerModalState {
    pub kind: String,
    pub filter: InputState,
    pub available: Vec<LeafFieldPath>,
    pub cursor: Option<String>,
    pub selected: BTreeSet<String>,
    /// Set of `group_path` strings currently collapsed in the tree view.
    pub collapsed_groups: BTreeSet<String>,
}

impl FieldPickerModalState {
    pub fn open_for(kind: &str, shown_paths: &[String]) -> Self {
        let all = yamlly_core::get_leaf_field_paths_cached(kind).unwrap_or_default();
        let mut available: Vec<LeafFieldPath> = all
            .iter()
            .filter(|leaf| !is_leaf_already_added(&leaf.path, shown_paths))
            .cloned()
            .collect();
        available.sort_by(|a, b| a.path.cmp(&b.path));

        // Expand top levels (depth 0..=2, matching ktui's `expand=(i <= 3)`) and
        // collapse deeper sub-trees initially so the tree stays compact and readable.
        let mut collapsed_groups = BTreeSet::new();
        for leaf in &available {
            let parts: Vec<&str> = leaf.path.split('.').collect();
            for i in 1..parts.len() {
                if i > 3 {
                    collapsed_groups.insert(parts[..i].join("."));
                }
            }
        }

        let mut state = Self {
            kind: kind.to_string(),
            filter: InputState::default(),
            available,
            cursor: None,
            selected: BTreeSet::new(),
            collapsed_groups,
        };
        state.cursor = state.tree_rows().first().map(|r| r.id().to_string());
        state
    }

    pub fn filtered(&self) -> Vec<&LeafFieldPath> {
        let q = self.filter.value().trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.available.iter().collect();
        }
        self.available
            .iter()
            .filter(|item| {
                item.path.to_ascii_lowercase().contains(&q)
                    || item.field_type.to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    /// Build the flattened visible rows of the hierarchical field picker tree.
    pub fn tree_rows(&self) -> Vec<FieldPickerRow> {
        let matching = self.filtered();
        let is_filtering = !self.filter.value().trim().is_empty();
        let mut root = PickerTreeBranch::new(String::new(), String::new());
        for leaf in matching {
            let parts: Vec<&str> = leaf.path.split('.').collect();
            root.insert(&parts, &parts, 0, leaf);
        }

        let mut out = Vec::new();
        // Top-level leaves (e.g. `data`, `type`, `automountServiceAccountToken`)
        for leaf in &root.leaves {
            let raw_name = leaf.path.rsplit('.').next().unwrap_or(&leaf.path);
            let clean_label = raw_name.strip_suffix("[]").unwrap_or(raw_name).to_string();
            out.push(FieldPickerRow::Leaf {
                path: leaf.path.clone(),
                label: clean_label,
                field_type: leaf.field_type.clone(),
                required: leaf.required,
                depth: 0,
                selected: self.selected.contains(&leaf.path),
            });
        }

        fn walk_branch(
            branch: &PickerTreeBranch<'_>,
            depth: usize,
            is_filtering: bool,
            collapsed_groups: &BTreeSet<String>,
            selected: &BTreeSet<String>,
            out: &mut Vec<FieldPickerRow>,
        ) {
            let (total_leaves, selected_leaves) = branch.counts(selected);
            if total_leaves == 0 {
                return;
            }
            let expanded = is_filtering || !collapsed_groups.contains(&branch.group_path);
            out.push(FieldPickerRow::Category {
                id: format!("{FIELD_PICKER_GROUP_PREFIX}{}", branch.group_path),
                group_path: branch.group_path.clone(),
                label: branch.name.clone(),
                depth,
                expanded,
                is_array: branch.name.ends_with("[]"),
                total_leaves,
                selected_leaves,
            });
            if expanded {
                for leaf in &branch.leaves {
                    let raw_name = leaf.path.rsplit('.').next().unwrap_or(&leaf.path);
                    let clean_label = raw_name.strip_suffix("[]").unwrap_or(raw_name).to_string();
                    out.push(FieldPickerRow::Leaf {
                        path: leaf.path.clone(),
                        label: clean_label,
                        field_type: leaf.field_type.clone(),
                        required: leaf.required,
                        depth: depth + 1,
                        selected: selected.contains(&leaf.path),
                    });
                }
                for child in &branch.children {
                    walk_branch(
                        child,
                        depth + 1,
                        is_filtering,
                        collapsed_groups,
                        selected,
                        out,
                    );
                }
            }
        }

        for child in &root.children {
            walk_branch(
                child,
                0,
                is_filtering,
                &self.collapsed_groups,
                &self.selected,
                &mut out,
            );
        }
        out
    }

    pub fn set_filter(&mut self, state: InputState) {
        let is_filtering = !state.value().trim().is_empty();
        self.filter = state;
        let rows = self.tree_rows();
        if is_filtering {
            // When searching, prefer keeping/placing the cursor on a matching leaf
            // so pressing Enter immediately adds the searched field.
            let current_is_matching_leaf = self.cursor.as_ref().is_some_and(|cur| {
                rows.iter()
                    .any(|r| matches!(r, FieldPickerRow::Leaf { path, .. } if path == cur))
            });
            if !current_is_matching_leaf {
                self.cursor = rows
                    .iter()
                    .find_map(|r| match r {
                        FieldPickerRow::Leaf { path, .. } => Some(path.clone()),
                        _ => None,
                    })
                    .or_else(|| rows.first().map(|r| r.id().to_string()));
            }
        } else {
            let visible_ids: Vec<String> = rows.iter().map(|r| r.id().to_string()).collect();
            if let Some(cur) = &self.cursor {
                if !visible_ids.contains(cur) {
                    self.cursor = visible_ids.first().cloned();
                }
            } else {
                self.cursor = visible_ids.first().cloned();
            }
        }
    }

    /// Toggle a row in the field picker tree:
    /// - On a `Category` row (`group:<path>`), toggles expanding / collapsing that category branch.
    /// - On a `Leaf` row (`<path>`), toggles selecting / deselecting that leaf field.
    pub fn toggle(&mut self, id: String) {
        self.cursor = Some(id.clone());
        if let Some(group_path) = id.strip_prefix(FIELD_PICKER_GROUP_PREFIX) {
            if !self.collapsed_groups.remove(group_path) {
                self.collapsed_groups.insert(group_path.to_string());
            }
        } else if !self.selected.remove(&id) {
            self.selected.insert(id);
        }
    }

    /// Select or deselect all available leaf fields under `group_path` (matching ktui's GROUP selection).
    /// Returns `true` if the group's leaves were selected, `false` if deselected.
    pub fn toggle_group_selection(&mut self, group_path: &str) -> bool {
        let prefix = format!("{group_path}.");
        let matching: Vec<String> = self
            .available
            .iter()
            .filter(|l| l.path == group_path || l.path.starts_with(&prefix))
            .map(|l| l.path.clone())
            .collect();
        if matching.is_empty() {
            return false;
        }
        let all_selected = matching.iter().all(|p| self.selected.contains(p));
        if all_selected {
            for p in matching {
                self.selected.remove(&p);
            }
            false
        } else {
            for p in matching {
                self.selected.insert(p);
            }
            self.collapsed_groups.remove(group_path);
            true
        }
    }

    /// Toggle selection of all leaves in the currently focused category (or the parent category of a focused leaf).
    pub fn toggle_focused_group_selection(&mut self) -> Option<(String, bool)> {
        let cur = self.cursor.clone()?;
        let group_path = if let Some(gp) = cur.strip_prefix(FIELD_PICKER_GROUP_PREFIX) {
            gp.to_string()
        } else {
            let (parent, _) = cur.rsplit_once('.')?;
            parent.to_string()
        };
        let selected = self.toggle_group_selection(&group_path);
        Some((group_path, selected))
    }

    /// Handle `Left` (`expand = false`) or `Right` (`expand = true`) arrow keys on the tree cursor.
    pub fn expand_or_collapse_cursor(&mut self, expand: bool) -> bool {
        let Some(cur) = self.cursor.clone() else {
            return false;
        };
        if let Some(gp) = cur.strip_prefix(FIELD_PICKER_GROUP_PREFIX) {
            if expand {
                return self.collapsed_groups.remove(gp);
            } else if !self.collapsed_groups.contains(gp) {
                self.collapsed_groups.insert(gp.to_string());
                return true;
            } else if let Some((parent, _)) = gp.rsplit_once('.') {
                self.cursor = Some(format!("{FIELD_PICKER_GROUP_PREFIX}{parent}"));
                return true;
            }
        } else if !expand && let Some((parent, _)) = cur.rsplit_once('.') {
            self.cursor = Some(format!("{FIELD_PICKER_GROUP_PREFIX}{parent}"));
            return true;
        }
        false
    }

    /// Return the leaf paths to add when committing the dialog.
    /// If checkboxes are selected, returns all checked leaf paths;
    /// otherwise, if the cursor is on a category node, returns all leaf paths in that category;
    /// otherwise returns the focused leaf path.
    pub fn paths_to_add(&self) -> Vec<String> {
        if !self.selected.is_empty() {
            self.selected.iter().cloned().collect()
        } else if let Some(cur) = &self.cursor {
            if let Some(group_path) = cur.strip_prefix(FIELD_PICKER_GROUP_PREFIX) {
                let prefix = format!("{group_path}.");
                self.available
                    .iter()
                    .filter(|l| l.path == group_path || l.path.starts_with(&prefix))
                    .map(|l| l.path.clone())
                    .collect()
            } else {
                vec![cur.clone()]
            }
        } else {
            Vec::new()
        }
    }
}

pub const DIR_PICKER_CURRENT_TOKEN: &str = ".";
pub const DIR_PICKER_PARENT_TOKEN: &str = "..";

/// Expand a leading `~` or `~/...` to the user's home directory if available.
pub fn expand_tilde(raw: &str) -> PathBuf {
    let trimmed = raw.trim();
    if trimmed == "~" {
        if let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
            return PathBuf::from(home);
        }
    } else if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
        && let Some(home) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE"))
    {
        return PathBuf::from(home).join(rest);
    }
    PathBuf::from(trimmed)
}

/// Format `path` cleanly by replacing the current user's home directory prefix
/// (`$HOME` or `%USERPROFILE%`) with `~`.
pub fn shorten_home_path(path: &Path) -> String {
    if let Some(home_os) = std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")) {
        let home = PathBuf::from(&home_os);
        let canon_home = std::fs::canonicalize(&home).unwrap_or_else(|_| home.clone());
        for candidate in [&canon_home, &home] {
            if let Ok(rel) = path.strip_prefix(candidate) {
                if rel.as_os_str().is_empty() {
                    return "~".to_string();
                }
                return format!("~/{}", rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    path.to_string_lossy().to_string()
}

/// Lexically normalize `.` and `..` path components without requiring the final leaf to exist.
pub fn normalize_lexical_path(path: &Path) -> PathBuf {
    use std::path::Component;
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::Prefix(p) => out.push(p.as_os_str()),
            Component::RootDir => out.push(comp.as_os_str()),
            Component::CurDir => {}
            Component::ParentDir => {
                if !matches!(
                    out.components().next_back(),
                    Some(Component::RootDir | Component::Prefix(_)) | None
                ) {
                    out.pop();
                }
            }
            Component::Normal(c) => out.push(c),
        }
    }
    if out.as_os_str().is_empty() {
        PathBuf::from("/")
    } else {
        out
    }
}

/// Return the absolute current working directory (falling back to `/`).
pub fn default_save_dir() -> PathBuf {
    std::env::current_dir()
        .map(|p| std::fs::canonicalize(&p).unwrap_or_else(|_| normalize_lexical_path(&p)))
        .unwrap_or_else(|_| PathBuf::from("/"))
}

/// Resolve any path into an existing absolute directory by walking up its ancestors if needed.
pub fn resolve_abs_dir(path: &Path) -> PathBuf {
    let raw_str = path.to_string_lossy();
    let expanded = expand_tilde(&raw_str);
    let abs = if expanded.is_absolute() {
        normalize_lexical_path(&expanded)
    } else {
        normalize_lexical_path(&default_save_dir().join(expanded))
    };

    for ancestor in abs.ancestors() {
        if !ancestor.as_os_str().is_empty() && ancestor.is_dir() {
            return std::fs::canonicalize(ancestor)
                .unwrap_or_else(|_| normalize_lexical_path(ancestor));
        }
    }
    default_save_dir()
}

/// State for the YAML Preview & Save [`ratcn::Dialog`].
#[derive(Clone)]
pub struct PreviewModalState {
    pub yaml: String,
    pub filename: String,
    pub path_input: InputState,
    pub scroll: u16,
}

impl Default for PreviewModalState {
    fn default() -> Self {
        let filename = "manifest.yaml".to_string();
        let default_path = shorten_home_path(&default_save_dir().join(&filename));
        Self {
            yaml: String::new(),
            filename,
            path_input: InputState::new(default_path),
            scroll: 0,
        }
    }
}

impl PreviewModalState {
    pub fn open_for(yaml: String, filename_or_path: String) -> Self {
        let is_path_like = filename_or_path.contains('/')
            || filename_or_path.contains('\\')
            || filename_or_path.starts_with('~');
        let (leaf_name, full_path) = if is_path_like {
            let expanded = expand_tilde(&filename_or_path);
            let abs = if expanded.is_absolute() {
                normalize_lexical_path(&expanded)
            } else {
                normalize_lexical_path(&default_save_dir().join(expanded))
            };
            let leaf = abs
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .filter(|s| !s.is_empty())
                .unwrap_or_else(|| "manifest.yaml".to_string());
            (leaf, abs)
        } else {
            let full = default_save_dir().join(&filename_or_path);
            (filename_or_path, full)
        };
        let display_path = shorten_home_path(&full_path);
        Self {
            yaml,
            filename: leaf_name,
            path_input: InputState::new(display_path),
            scroll: 0,
        }
    }

    /// Extract the filename currently in `path_input` (if any), falling back to `self.filename`.
    pub fn effective_filename(&self) -> String {
        let raw = self.path_input.value().trim();
        if !raw.is_empty() && !raw.ends_with('/') && !raw.ends_with('\\') {
            let expanded = expand_tilde(raw);
            if !expanded.is_dir()
                && let Some(name) = expanded.file_name()
            {
                let s = name.to_string_lossy().trim().to_string();
                if !s.is_empty() && s != "." && s != ".." {
                    return s;
                }
            }
        }
        self.filename.clone()
    }

    pub fn set_directory(&mut self, dir: &Path) {
        let fname = self.effective_filename();
        self.filename = fname.clone();
        let abs_dir = resolve_abs_dir(dir);
        let full = shorten_home_path(&abs_dir.join(&fname));
        self.path_input = InputState::new(full);
    }

    /// Resolve the user's typed path into an absolute target file path anywhere on the system.
    pub fn resolve_save_path(&self) -> Option<PathBuf> {
        let raw = self.path_input.value().trim();
        if raw.is_empty() {
            return None;
        }
        let expanded = expand_tilde(raw);
        let abs = if expanded.is_absolute() {
            normalize_lexical_path(&expanded)
        } else {
            normalize_lexical_path(&default_save_dir().join(expanded))
        };
        let ends_with_sep = raw.ends_with('/') || raw.ends_with('\\');
        if ends_with_sep || abs.is_dir() {
            Some(abs.join(&self.filename))
        } else {
            Some(abs)
        }
    }
}

/// State for the Directory Picker [`ratcn::Dialog`].
#[derive(Debug, Clone)]
pub struct DirPickerModalState {
    pub current_dir: PathBuf,
    pub entries: Vec<PathBuf>,
    pub cursor: Option<String>,
}

impl Default for DirPickerModalState {
    fn default() -> Self {
        Self::open_at(default_save_dir())
    }
}

impl DirPickerModalState {
    pub fn open_at(dir: PathBuf) -> Self {
        let current_dir = resolve_abs_dir(&dir);
        let mut me = Self {
            current_dir,
            entries: Vec::new(),
            cursor: None,
        };
        me.reload();
        me
    }

    pub fn has_parent(&self) -> bool {
        self.current_dir
            .parent()
            .is_some_and(|p| !p.as_os_str().is_empty() && p != self.current_dir)
    }

    pub fn reload(&mut self) {
        let mut dirs = Vec::new();
        if let Ok(read) = std::fs::read_dir(&self.current_dir) {
            for entry in read.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    let name = p
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if !name.starts_with('.') {
                        dirs.push(p);
                    }
                }
            }
        }
        dirs.sort();
        self.cursor = Some(DIR_PICKER_CURRENT_TOKEN.to_string());
        self.entries = dirs;
    }

    pub fn go_up(&mut self) {
        if let Some(parent) = self.current_dir.parent().map(Path::to_path_buf)
            && !parent.as_os_str().is_empty()
            && parent != self.current_dir
        {
            self.current_dir = parent;
            self.reload();
        }
    }

    /// Handle activating (`Enter`) an entry in the directory picker list.
    /// Returns `true` if the current directory `.` was chosen to commit selection.
    pub fn enter_named(&mut self, name: &str) -> bool {
        if name == DIR_PICKER_CURRENT_TOKEN {
            return true;
        }
        if name == DIR_PICKER_PARENT_TOKEN {
            self.go_up();
            return false;
        }
        let next = self.current_dir.join(name);
        if next.is_dir() {
            self.current_dir = resolve_abs_dir(&next);
            self.reload();
        }
        false
    }

    /// Return the directory to commit when the user clicks "Select Directory".
    pub fn selected_dir(&self) -> PathBuf {
        match self.cursor.as_deref() {
            None | Some(DIR_PICKER_CURRENT_TOKEN) | Some(DIR_PICKER_PARENT_TOKEN) => {
                self.current_dir.clone()
            }
            Some(sub) => {
                let candidate = self.current_dir.join(sub);
                if candidate.is_dir() {
                    resolve_abs_dir(&candidate)
                } else {
                    self.current_dir.clone()
                }
            }
        }
    }
}

/// One key-value row inside [`MapEditorModalState`].
#[derive(Clone)]
pub struct MapEntryRow {
    pub id: ChildId,
    pub key: InputState,
    pub value: InputState,
}

/// State for the Key/Value Map Editor [`ratcn::Dialog`].
#[derive(Clone, Default)]
pub struct MapEditorModalState {
    pub path: String,
    pub title: String,
    pub rows: Vec<MapEntryRow>,
    pub scroll: u16,
    pub next_seq: usize,
}

impl MapEditorModalState {
    pub fn open_for(path: &str, title: String, entries: Vec<(String, String)>) -> Self {
        let mut me = Self {
            path: path.to_string(),
            title,
            rows: Vec::new(),
            scroll: 0,
            next_seq: 0,
        };
        if entries.is_empty() {
            me.add_row(String::new(), String::new());
        } else {
            for (k, v) in entries {
                me.add_row(k, v);
            }
        }
        me
    }

    pub fn add_row(&mut self, key: String, value: String) {
        self.next_seq += 1;
        let id = ChildId::from(format!("map-row-{}", self.next_seq));
        self.rows.push(MapEntryRow {
            id,
            key: InputState::new(key),
            value: InputState::new(value),
        });
    }

    pub fn remove_row(&mut self, idx: usize) {
        if idx < self.rows.len() {
            self.rows.remove(idx);
        }
        if self.rows.is_empty() {
            self.add_row(String::new(), String::new());
        }
    }

    pub fn collect_entries(&self) -> Vec<(String, String)> {
        self.rows
            .iter()
            .filter_map(|r| {
                let k = r.key.value().trim().to_string();
                let v = r.value.value().to_string();
                if k.is_empty() { None } else { Some((k, v)) }
            })
            .collect()
    }

    /// Merge imported `(key, value)` pairs into the current map editor rows.
    pub fn merge_entries(&mut self, imported: Vec<(String, String)>) {
        // Remove the default empty placeholder row if it is the only row and still blank
        if self.rows.len() == 1
            && self.rows[0].key.value().trim().is_empty()
            && self.rows[0].value.value().trim().is_empty()
        {
            self.rows.clear();
        }
        for (k, v) in imported {
            if let Some(existing) = self.rows.iter_mut().find(|r| r.key.value().trim() == k) {
                existing.value = InputState::new(v);
            } else {
                self.add_row(k, v);
            }
        }
        if self.rows.is_empty() {
            self.add_row(String::new(), String::new());
        }
    }
}

/// State for the Enum / Namespace Dropdown List [`ratcn::Dialog`].
#[derive(Debug, Clone, Default)]
pub struct SelectPickerModalState {
    pub path: String,
    pub title: String,
    pub options: Vec<String>,
    pub cursor: Option<String>,
    pub selected: Option<String>,
}

impl SelectPickerModalState {
    pub fn open_for(
        path: &str,
        title: String,
        options: Vec<String>,
        current: Option<String>,
    ) -> Self {
        let cursor = current
            .clone()
            .filter(|c| options.contains(c))
            .or_else(|| options.first().cloned());
        Self {
            path: path.to_string(),
            title,
            options,
            cursor,
            selected: current,
        }
    }
}

/// State for the `ktui`-style Cluster Import [`ratcn::Dialog`].
#[derive(Clone)]
pub struct ClusterImportModalState {
    /// Target map field path (e.g. `"metadata.labels"`, `"spec.selector.matchLabels"`),
    /// or `None` when importing a full resource manifest into a new editor tab.
    pub target_path: Option<String>,
    /// True when opened from inside `MapEditorModal` so imported pairs merge into its rows.
    pub into_map_modal: bool,
    /// Currently selected cluster resource kind (`"Deployment"`, `"Pod"`, `"Service"`, etc.).
    pub selected_kind: String,
    /// Live search filter input for resource name/namespace.
    pub filter: InputState,
    /// True while an async `kubectl get <kind> -o json` request is in flight.
    pub is_fetching: bool,
    /// Status banner text (`"Fetching Deployments..."`, `"Found 3 resources."`, etc.).
    pub status_message: String,
    /// Raw JSON items returned from `kubectl get <kind> -o json`.
    pub items: Vec<serde_json::Value>,
    /// Focused item index in `items` (stored as stringified index `"0"`, `"1"`, ...).
    pub cursor: Option<String>,
}

impl Default for ClusterImportModalState {
    fn default() -> Self {
        Self {
            target_path: Some("metadata.labels".to_string()),
            into_map_modal: false,
            selected_kind: "Deployment".to_string(),
            filter: InputState::default(),
            is_fetching: false,
            status_message: String::new(),
            items: Vec::new(),
            cursor: None,
        }
    }
}

impl ClusterImportModalState {
    pub const KINDS: &'static [&'static str] = &[
        "Deployment",
        "Pod",
        "Service",
        "StatefulSet",
        "DaemonSet",
        "ConfigMap",
        "Secret",
        "Ingress",
        "Job",
        "CronJob",
    ];

    pub fn open_for_map(path: &str, into_map_modal: bool) -> Self {
        Self {
            target_path: Some(path.to_string()),
            into_map_modal,
            selected_kind: "Deployment".to_string(),
            filter: InputState::default(),
            is_fetching: true,
            status_message: "Fetching Deployments from cluster...".to_string(),
            items: Vec::new(),
            cursor: None,
        }
    }

    pub fn open_for_manifest(default_kind: &str) -> Self {
        let kind = if Self::KINDS.contains(&default_kind) {
            default_kind.to_string()
        } else {
            "Deployment".to_string()
        };
        Self {
            target_path: None,
            into_map_modal: false,
            status_message: format!("Fetching {kind}s from cluster..."),
            selected_kind: kind,
            filter: InputState::default(),
            is_fetching: true,
            items: Vec::new(),
            cursor: None,
        }
    }

    pub fn set_items(&mut self, items: Vec<serde_json::Value>, error: Option<String>) {
        self.is_fetching = false;
        self.items = items;
        if let Some(err) = error {
            self.status_message = format!("Cluster error: {err}");
            self.cursor = None;
        } else if self.items.is_empty() {
            self.status_message = format!("No {} resources found in cluster.", self.selected_kind);
            self.cursor = None;
        } else {
            self.status_message = format!(
                "Found {} {} resource(s) — press Enter or Click to import:",
                self.items.len(),
                self.selected_kind
            );
            let filtered = self.filtered_indices();
            self.cursor = filtered.first().map(|i| i.to_string());
        }
    }

    pub fn filtered_indices(&self) -> Vec<usize> {
        let q = self.filter.value().trim().to_ascii_lowercase();
        if q.is_empty() {
            return (0..self.items.len()).collect();
        }
        (0..self.items.len())
            .filter(|&i| {
                let item = &self.items[i];
                let name = item
                    .pointer("/metadata/name")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                let ns = item
                    .pointer("/metadata/namespace")
                    .and_then(|v| v.as_str())
                    .unwrap_or("");
                name.to_ascii_lowercase().contains(&q)
                    || ns.to_ascii_lowercase().contains(&q)
                    || format!("{ns}/{name}").to_ascii_lowercase().contains(&q)
            })
            .collect()
    }

    pub fn selected_index(&self) -> Option<usize> {
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            return None;
        }
        if let Some(cur) = &self.cursor
            && let Ok(idx) = cur.split(':').next().unwrap_or("").parse::<usize>()
            && filtered.contains(&idx)
        {
            return Some(idx);
        }
        filtered.first().copied()
    }

    pub fn select_next(&mut self, forward: bool) {
        let filtered = self.filtered_indices();
        if filtered.is_empty() {
            self.cursor = None;
            return;
        }
        let cur_idx_opt = self
            .cursor
            .as_ref()
            .and_then(|c| c.split(':').next().and_then(|s| s.parse::<usize>().ok()));
        let cur_pos = cur_idx_opt
            .and_then(|idx| filtered.iter().position(|&x| x == idx))
            .unwrap_or(0);
        let next_pos = if forward {
            (cur_pos + 1) % filtered.len()
        } else {
            (cur_pos + filtered.len() - 1) % filtered.len()
        };
        self.cursor = Some(filtered[next_pos].to_string());
    }

    pub fn dialog_title(&self) -> String {
        match &self.target_path {
            Some(path) => {
                let prop = if path.to_ascii_lowercase().contains("selector") {
                    "selector labels"
                } else if path.to_ascii_lowercase().contains("annotation") {
                    "annotations"
                } else {
                    "labels"
                };
                format!("Import {prop} from Cluster — {path}")
            }
            None => "Import Resource Manifest from Cluster".to_string(),
        }
    }
}

/// State for the Kubernetes Context Switcher [`ratcn::Dialog`].
#[derive(Clone, Default)]
pub struct ContextSwitcherModalState {
    pub current_context: Option<String>,
    pub available_contexts: Vec<String>,
    pub filter: InputState,
    pub cursor: Option<String>,
}

impl ContextSwitcherModalState {
    pub fn open_for(current: Option<String>, available: Vec<String>) -> Self {
        let cursor = if let Some(cur) = &current {
            if available.contains(cur) {
                Some(cur.clone())
            } else {
                available.first().cloned()
            }
        } else {
            available.first().cloned()
        };
        Self {
            current_context: current,
            available_contexts: available,
            filter: InputState::default(),
            cursor,
        }
    }

    pub fn filtered_contexts(&self) -> Vec<String> {
        let q = self.filter.value().trim().to_ascii_lowercase();
        if q.is_empty() {
            return self.available_contexts.clone();
        }
        self.available_contexts
            .iter()
            .filter(|c| c.to_ascii_lowercase().contains(&q))
            .cloned()
            .collect()
    }

    pub fn selected_context(&self) -> Option<String> {
        self.cursor
            .clone()
            .or_else(|| self.filtered_contexts().first().cloned())
    }

    pub fn select_next(&mut self, forward: bool) {
        let list = self.filtered_contexts();
        if list.is_empty() {
            self.cursor = None;
            return;
        }
        let cur_idx = self
            .cursor
            .as_ref()
            .and_then(|c| list.iter().position(|x| x == c))
            .unwrap_or(0);
        let next_idx = if forward {
            (cur_idx + 1) % list.len()
        } else {
            (cur_idx + list.len() - 1) % list.len()
        };
        self.cursor = Some(list[next_idx].clone());
    }
}
