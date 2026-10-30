//! Per-tab form state: owns the document being edited, the built rows, and
//! the keyboard focus/edit state machine.
//!
//! All mutation funnels through [`FormState::set_field_value`], which writes
//! into the document with `set_nested` and refreshes the dirty flag.

use std::collections::{HashMap, HashSet};

use ratcn::InputState;
use serde_json::{Map, Value, json};
use yamlly_core::field::{FieldKind, FormField, Row};
use yamlly_core::{get_nested, normalize, set_nested};

/// One row as it will be drawn, with the state needed to render it.
#[derive(Debug, Clone)]
pub struct RowView {
    pub kind: Row,
    pub depth: usize,
    /// Value currently stored at this field's path, rendered as text.
    pub display: String,
    /// Validation message, if any.
    pub error: Option<String>,
}

/// Editing/focus state.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    /// Navigating between fields.
    Navigate,
    /// Editing a textual field's buffer.
    Edit,
    /// Cycling an enum/namespace dropdown.
    Select,
}

#[derive(Debug)]
pub struct FormState {
    pub kind: String,
    pub api_version: String,
    /// The document under edit.
    pub data: Value,
    /// Normalized snapshot taken at load / last save, for dirty tracking.
    pub baseline: Value,
    pub rows: Vec<RowView>,
    pub focus: usize,
    pub mode: Mode,
    /// Cursor position within the edit buffer.
    pub cursor: usize,
    /// Active edit buffer; committed on Enter, discarded on Esc.
    pub buffer: String,
    /// Preserved value when editing began, so Esc restores it accurately.
    pub saved_value: Option<Value>,
    /// Number of items materialised per array path.
    pub array_counts: HashMap<String, usize>,
    /// Paths already rendered (the field picker filters on this).
    pub shown_paths: Vec<String>,
    pub namespaces: Vec<String>,
    /// Per-field ratcn InputState for interactive single-line editing.
    pub inputs: HashMap<String, InputState>,
    /// Fallback empty InputState for borrowing when a path is absent.
    pub empty_input: InputState,
    /// Set of collapsed category keys (`sec:<name>`, `grp:<path>`, `arr:<path>`).
    pub collapsed_categories: HashSet<String>,
    /// Extra schema paths merged in beyond the kind's common fields (the
    /// field picker's selections).
    extra_paths: Vec<String>,
    pub express_mode: bool,
    scroll: usize,
}

impl FormState {
    /// Build a form for `kind`, optionally seeded from an existing document.
    pub fn new(kind: &str, initial: Option<&Value>, namespaces: Vec<String>) -> Self {
        let (api_version, data) = match yamlly_core::load_resource_ref(kind) {
            Some(res) => {
                let api = res
                    .get("apiVersion")
                    .and_then(|v| v.as_str())
                    .unwrap_or("v1")
                    .to_string();
                let mut data = Map::new();
                data.insert("apiVersion".into(), json!(api));
                data.insert("kind".into(), json!(kind));
                if let Some(Value::Object(src)) = initial {
                    for (k, v) in src {
                        if k != "apiVersion" && k != "kind" {
                            data.insert(k.clone(), v.clone());
                        }
                    }
                }
                (api, Value::Object(data))
            }
            None => (String::new(), Value::Object(Map::new())),
        };

        let mut extra_paths = Vec::new();
        let mut array_counts = HashMap::new();
        if initial.is_some() {
            let schema_fields = yamlly_core::load_resource_ref(kind).and_then(|r| r.get("fields"));
            collect_data_paths(
                &data,
                "",
                schema_fields,
                &mut extra_paths,
                &mut array_counts,
            );
        }

        let mut me = Self {
            kind: kind.to_string(),
            api_version,
            baseline: Value::Null,
            data,
            rows: Vec::new(),
            focus: 0,
            mode: Mode::Navigate,
            cursor: 0,
            buffer: String::new(),
            saved_value: None,
            array_counts,
            shown_paths: Vec::new(),
            namespaces,
            inputs: HashMap::new(),
            empty_input: InputState::default(),
            collapsed_categories: HashSet::new(),
            extra_paths,
            express_mode: false,
            scroll: 0,
        };
        // Baseline comes from the merged data, so an opened file that already
        // matches its own schema does not open dirty.
        me.baseline = normalize(&me.data).unwrap_or(Value::Object(Map::new()));
        me.rebuild();
        me
    }

    /// Rebuild rows from the schema. Array counts and extra paths persist.
    pub fn rebuild(&mut self) {
        self.rebuild_with_extra(&[]);
    }

    /// Rebuild with additional schema paths merged in. Preserves any paths
    /// already tracked in `extra_paths`.
    pub fn rebuild_with_extra(&mut self, extra: &[String]) {
        for p in extra {
            if !self.extra_paths.contains(p) {
                self.extra_paths.push(p.clone());
            }
        }
        let (rows, shown) = yamlly_core::build_form_with_mode(
            &self.kind,
            &self.namespaces,
            &self.extra_paths,
            &self.array_counts,
            self.express_mode,
        )
        .unwrap_or_default();
        self.shown_paths = shown;
        self.rows = rows
            .into_iter()
            .map(|kind| {
                let display = match &kind {
                    Row::Field { field, .. } => self.render_value(field),
                    _ => String::new(),
                };
                if let Row::Field { field, .. } = &kind {
                    let needs_sync = self
                        .inputs
                        .get(&field.path)
                        .map(|s| s.value() != display.as_str())
                        .unwrap_or(true);
                    if needs_sync {
                        self.inputs
                            .insert(field.path.clone(), InputState::new(display.clone()));
                    }
                }
                RowView {
                    // Carry the layout depth through: the renderer indents from
                    // this, not from the nested Row.
                    depth: kind.depth(),
                    kind,
                    display,
                    error: None,
                }
            })
            .collect();
        if self.focus >= self.rows.len() {
            self.focus = self.rows.len().saturating_sub(1);
        }

        if !extra.is_empty() {
            // Uncollapse any ancestor categories of the newly added paths so they are immediately visible.
            let mut to_uncollapse = Vec::new();
            for i in 0..self.rows.len() {
                if let Some(cat_key) = self.category_key_at(i)
                    && self.collapsed_categories.contains(&cat_key)
                {
                    let is_ancestor = match &self.rows[i].kind {
                        Row::Section(s) => extra
                            .iter()
                            .any(|p| p == s || p.starts_with(&format!("{s}."))),
                        Row::Group { .. } => {
                            if let Some(rest) = cat_key.strip_prefix("grp:") {
                                let dotted = rest.replace(':', ".");
                                extra
                                    .iter()
                                    .any(|p| p == &dotted || p.starts_with(&format!("{dotted}.")))
                            } else {
                                false
                            }
                        }
                        Row::Array { path, .. } => {
                            let clean = path.strip_suffix("[]").unwrap_or(path);
                            extra.iter().any(|p| {
                                let p_clean = p.strip_suffix("[]").unwrap_or(p);
                                p_clean == clean
                                    || p_clean.starts_with(&format!("{clean}."))
                                    || p_clean.starts_with(&format!("{clean}["))
                            })
                        }
                        Row::Field { .. } => false,
                    };
                    if is_ancestor {
                        to_uncollapse.push(cat_key);
                    }
                }
            }
            for k in to_uncollapse {
                self.collapsed_categories.remove(&k);
            }

            // Focus the first newly added field and scroll it into view.
            if let Some(idx) = self.rows.iter().position(|r| match &r.kind {
                Row::Field { field, .. } => {
                    let f_norm = normalize_for_match(&field.path);
                    let f_strip = f_norm.strip_suffix("[]").unwrap_or(&f_norm);
                    extra.iter().any(|p| {
                        let p_norm = normalize_for_match(p);
                        let p_strip = p_norm.strip_suffix("[]").unwrap_or(&p_norm);
                        f_strip == p_strip
                            || f_strip.starts_with(&format!("{p_strip}."))
                            || p_strip.starts_with(&format!("{f_strip}."))
                    })
                }
                _ => false,
            }) {
                self.focus = idx;
                self.ensure_visible(15);
            }
        }
    }

    /// Current row if it is an editable field.
    pub fn focused_field(&self) -> Option<&FormField> {
        match self.rows.get(self.focus).map(|r| &r.kind) {
            Some(Row::Field { field, .. }) => Some(field),
            _ => None,
        }
    }

    /// Move focus to an arbitrary row (e.g. from a mouse click).
    ///
    /// Only field rows take focus; clicking a header does nothing so the
    /// selection never lands somewhere un-navigable.
    pub fn focus_row(&mut self, idx: usize) {
        if matches!(self.rows.get(idx).map(|r| &r.kind), Some(Row::Field { .. })) {
            self.focus = idx;
        }
    }

    fn field_index(&self) -> Option<usize> {
        matches!(
            self.rows.get(self.focus).map(|r| &r.kind),
            Some(Row::Field { .. })
        )
        .then_some(self.focus)
    }

    /// Look up a field's current value. Paths carry `[N]` array indices, so
    /// they must be split before `get_nested` sees them.
    pub fn field_value(&self, path: &str) -> Option<&Value> {
        let segments = split_data_path(path);
        get_nested(&self.data, &segments)
    }

    /// Render a field's stored value as display text.
    fn render_value(&self, field: &FormField) -> String {
        let Some(v) = self.field_value(&field.path) else {
            return String::new();
        };
        match field.kind {
            FieldKind::Boolean => match v {
                Value::Bool(b) => b.to_string(),
                Value::Null => "false".into(),
                other => other.to_string(),
            },
            FieldKind::Map => match v {
                Value::Object(m) if !m.is_empty() => {
                    let n = m.len();
                    format!("{n} {}  [+]", if n == 1 { "entry" } else { "entries" })
                }
                _ => "[+] add key/value".into(),
            },
            FieldKind::StringList => match v {
                Value::Array(arr) => arr
                    .iter()
                    .map(value_to_display)
                    .collect::<Vec<_>>()
                    .join(", "),
                _ => value_to_display(v),
            },
            _ => value_to_display(v),
        }
    }

    /// Write a value at a field's path and refresh derived state.
    pub fn set_field_value(&mut self, field: &FormField, value: Value) {
        let path: Vec<String> = split_data_path(&field.path);
        set_nested(&mut self.data, &path, value);
        self.refresh_row(self.field_index().unwrap_or(0));
    }

    fn refresh_row(&mut self, idx: usize) {
        let Some((path, text)) = self.rows.get(idx).and_then(|r| match &r.kind {
            Row::Field { field, .. } => Some((field.path.clone(), self.render_value(field))),
            _ => None,
        }) else {
            return;
        };
        if let Some(rv) = self.rows.get_mut(idx) {
            rv.display = text.clone();
        }
        if self.inputs.get(&path).map(|s| s.value()) != Some(text.as_str()) {
            self.inputs.insert(path, InputState::new(text));
        }
    }

    /// Borrow the `ratcn::InputState` for `path`, falling back to `empty_input`.
    pub fn input_state(&self, path: &str) -> &InputState {
        self.inputs.get(path).unwrap_or(&self.empty_input)
    }

    /// Refresh the display text and input buffer for all rows presenting `target_path`.
    pub fn refresh_path(&mut self, target_path: &str) {
        let indices: Vec<usize> = self
            .rows
            .iter()
            .enumerate()
            .filter_map(|(i, r)| match &r.kind {
                Row::Field { field, .. } if field.path == target_path => Some(i),
                _ => None,
            })
            .collect();
        for idx in indices {
            self.refresh_row(idx);
        }
    }

    /// Handle a change emitted by a `ratcn::Input` at `row_idx`.
    pub fn set_input_state(&mut self, row_idx: usize, state: InputState) {
        let Some(field) = self.rows.get(row_idx).and_then(|r| match &r.kind {
            Row::Field { field, .. } => Some(field.clone()),
            _ => None,
        }) else {
            return;
        };
        self.focus = row_idx;
        let raw = state.value().to_string();
        self.inputs.insert(field.path.clone(), state);
        let value = coerce(&raw, &field.kind);
        let path: Vec<String> = split_data_path(&field.path);
        set_nested(&mut self.data, &path, value);
        let text = self.render_value(&field);
        if let Some(rv) = self.rows.get_mut(row_idx) {
            rv.display = text;
            rv.error = None;
        }
    }

    /// Handle a change emitted by a `ratcn::Checkbox` at `row_idx`.
    pub fn set_boolean_at(&mut self, row_idx: usize, checked: bool) {
        let Some(field) = self.rows.get(row_idx).and_then(|r| match &r.kind {
            Row::Field { field, .. } => Some(field.clone()),
            _ => None,
        }) else {
            return;
        };
        self.focus = row_idx;
        let path: Vec<String> = split_data_path(&field.path);
        set_nested(&mut self.data, &path, Value::Bool(checked));
        self.refresh_row(row_idx);
    }

    /// Return the option list for an Enum or Namespace field.
    pub fn options_for(&self, field: &FormField) -> Vec<String> {
        match &field.kind {
            FieldKind::Enum(vals) => vals.clone(),
            FieldKind::Namespace => {
                if self.namespaces.is_empty() {
                    vec![
                        "default".to_string(),
                        "kube-system".to_string(),
                        "kube-public".to_string(),
                    ]
                } else {
                    self.namespaces.clone()
                }
            }
            _ => Vec::new(),
        }
    }

    /// Return the index of the currently selected option for `Cycle` (0 = "(unset)", 1..=N = options).
    pub fn option_index_at(&self, row_idx: usize) -> usize {
        let Some(field) = self.rows.get(row_idx).and_then(|r| match &r.kind {
            Row::Field { field, .. } => Some(field),
            _ => None,
        }) else {
            return 0;
        };
        let options = self.options_for(field);
        let current = self
            .field_value(&field.path)
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if current.is_empty() {
            return 0;
        }
        options
            .iter()
            .position(|o| o == current)
            .map(|i| i + 1)
            .unwrap_or(0)
    }

    /// Set option index from `Cycle` (0 = unset, 1..=N = option).
    pub fn set_option_index_at(&mut self, row_idx: usize, idx: usize) {
        let Some(field) = self.rows.get(row_idx).and_then(|r| match &r.kind {
            Row::Field { field, .. } => Some(field.clone()),
            _ => None,
        }) else {
            return;
        };
        self.focus = row_idx;
        let options = self.options_for(&field);
        let val = if idx == 0 {
            None
        } else {
            options.get(idx - 1).cloned()
        };
        self.set_select_value(&field.path, val);
    }

    // -----------------------------------------------------------------
    // Navigation
    // -----------------------------------------------------------------

    /// Move focus to the next/previous visible field, skipping headers and collapsed rows.
    pub fn focus_next_field(&mut self, forward: bool) {
        self.focus_next_field_from(self.focus, forward);
    }

    /// As [`FormState::focus_next_field`], but starting the search at `from`.
    pub fn focus_next_field_from(&mut self, from: usize, forward: bool) {
        if self.rows.is_empty() {
            return;
        }
        let vis = self.row_visibility();
        let len = self.rows.len();
        for step in 1..=len {
            let idx = if forward {
                (from + step) % len
            } else {
                (from + len - (step % len)) % len
            };
            if vis.get(idx).copied().unwrap_or(true)
                && matches!(self.rows[idx].kind, Row::Field { .. })
            {
                self.focus = idx;
                return;
            }
        }
    }

    /// Unique category key for a `Section`, `Group`, or `Array` row at `row_idx`.
    pub fn category_key_at(&self, row_idx: usize) -> Option<String> {
        let mut current_sec = String::new();
        let mut group_stack: Vec<(usize, String)> = Vec::new();
        for (i, r) in self.rows.iter().enumerate() {
            match &r.kind {
                Row::Section(s) => {
                    current_sec = s.clone();
                    group_stack.clear();
                    if i == row_idx {
                        return Some(format!("sec:{current_sec}"));
                    }
                }
                Row::Group { label, depth } => {
                    group_stack.retain(|(d, _)| *d < *depth);
                    group_stack.push((*depth, label.clone()));
                    if i == row_idx {
                        let path: Vec<&str> = group_stack.iter().map(|(_, l)| l.as_str()).collect();
                        return Some(format!("grp:{current_sec}:{}", path.join(".")));
                    }
                }
                Row::Array { path, depth, .. } => {
                    group_stack.retain(|(d, _)| *d < *depth);
                    group_stack.push((*depth, path.clone()));
                    if i == row_idx {
                        return Some(format!("arr:{path}"));
                    }
                }
                Row::Field { .. } => {
                    if i == row_idx {
                        return None;
                    }
                }
            }
        }
        None
    }

    /// True if the category header (`Section`, `Group`, or `Array`) at `row_idx` is collapsed.
    pub fn is_category_collapsed(&self, row_idx: usize) -> bool {
        if self.collapsed_categories.is_empty() {
            return false;
        }
        self.category_key_at(row_idx)
            .is_some_and(|k| self.collapsed_categories.contains(&k))
    }

    /// Toggle the collapsed state of the category header (`Section`, `Group`, or `Array`) at `row_idx`.
    /// Returns `true` if `row_idx` was a collapsible category header.
    pub fn toggle_category_at(&mut self, row_idx: usize) -> bool {
        let Some(key) = self.category_key_at(row_idx) else {
            return false;
        };
        if !self.collapsed_categories.remove(&key) {
            self.collapsed_categories.insert(key);
        }
        let vis = self.row_visibility();
        if !vis.get(self.focus).copied().unwrap_or(true) {
            self.focus_next_field_from(row_idx, true);
        }
        true
    }

    /// Compute visibility for every row in `self.rows`: `true` if visible, `false` if hidden inside a collapsed category.
    pub fn row_visibility(&self) -> Vec<bool> {
        let mut vis = vec![true; self.rows.len()];
        if self.collapsed_categories.is_empty() {
            return vis;
        }
        let mut section_collapsed = false;
        let mut collapsed_depth: Option<usize> = None;

        for (i, r) in self.rows.iter().enumerate() {
            match &r.kind {
                Row::Section(_) => {
                    collapsed_depth = None;
                    section_collapsed = self.is_category_collapsed(i);
                    vis[i] = true;
                }
                Row::Group { depth, .. } | Row::Array { depth, .. } => {
                    if section_collapsed {
                        vis[i] = false;
                        continue;
                    }
                    if let Some(cd) = collapsed_depth {
                        if *depth > cd {
                            vis[i] = false;
                            continue;
                        } else {
                            collapsed_depth = None;
                        }
                    }
                    vis[i] = true;
                    if self.is_category_collapsed(i) {
                        collapsed_depth = Some(*depth);
                    }
                }
                Row::Field { depth, .. } => {
                    if section_collapsed {
                        vis[i] = false;
                        continue;
                    }
                    if let Some(cd) = collapsed_depth {
                        if *depth > cd {
                            vis[i] = false;
                            continue;
                        } else {
                            collapsed_depth = None;
                        }
                    }
                    vis[i] = true;
                }
            }
        }
        vis
    }

    /// Keep the focused row inside the viewport. Rows have variable height, so
    /// this works in virtual lines rather than row indices.
    pub fn ensure_visible(&mut self, height: usize) {
        let height = height.max(1);
        let (start, end) = self.focus_lines();
        if start < self.scroll {
            self.scroll = start;
        } else if end > self.scroll + height {
            self.scroll = end.saturating_sub(height);
        }
    }

    pub fn scroll(&self) -> usize {
        self.scroll
    }

    pub fn set_scroll(&mut self, offset: usize) {
        self.scroll = offset;
    }

    // -----------------------------------------------------------------
    // Editing
    // -----------------------------------------------------------------

    /// Begin editing (or cycling) the focused field.
    pub fn begin_edit(&mut self) {
        let Some(idx) = self.field_index() else {
            return;
        };
        let Some(field) = self.focused_field().cloned() else {
            return;
        };
        // Boolean toggles immediately on Enter (no edit mode), matching the
        // original's Switch widget.
        if matches!(field.kind, FieldKind::Boolean) {
            self.toggle_boolean();
            self.mode = Mode::Navigate;
            return;
        }
        if matches!(field.kind, FieldKind::Enum(_) | FieldKind::Namespace) {
            self.mode = Mode::Select;
            // Seed the first option so the field is not blank on entry.
            self.cycle_option(true);
            return;
        }
        self.saved_value = self.field_value(&field.path).cloned();
        self.buffer = self.rows[idx].display.clone();
        self.cursor = self.buffer.chars().count();
        self.mode = Mode::Edit;
    }

    /// Synchronize the current edit buffer to `self.data` immediately.
    pub fn sync_live_value(&mut self) {
        if self.mode != Mode::Edit {
            return;
        }
        let Some(field) = self.focused_field().cloned() else {
            return;
        };
        let value = coerce(&self.buffer, &field.kind);
        let path: Vec<String> = split_data_path(&field.path);
        set_nested(&mut self.data, &path, value);
        if let Some(idx) = self.field_index() {
            self.refresh_row(idx);
        }
    }

    pub fn insert_char(&mut self, c: char) {
        if self.mode != Mode::Edit {
            return;
        }
        let byte = char_to_byte(&self.buffer, self.cursor);
        self.buffer.insert(byte, c);
        self.cursor += 1;
        self.sync_live_value();
    }

    pub fn delete_char(&mut self) {
        if self.mode != Mode::Edit || self.cursor == 0 {
            return;
        }
        let start = char_to_byte(&self.buffer, self.cursor - 1);
        let end = char_to_byte(&self.buffer, self.cursor);
        self.buffer.replace_range(start..end, "");
        self.cursor -= 1;
        self.sync_live_value();
    }

    pub fn delete_char_forward(&mut self) {
        if self.mode != Mode::Edit || self.cursor >= self.buffer.chars().count() {
            return;
        }
        let start = char_to_byte(&self.buffer, self.cursor);
        let end = char_to_byte(&self.buffer, self.cursor + 1);
        self.buffer.replace_range(start..end, "");
        self.sync_live_value();
    }

    pub fn move_cursor_home(&mut self) {
        if self.mode == Mode::Edit {
            self.cursor = 0;
        }
    }

    pub fn move_cursor_end(&mut self) {
        if self.mode == Mode::Edit {
            self.cursor = self.buffer.chars().count();
        }
    }

    pub fn move_cursor(&mut self, forward: bool) {
        match self.mode {
            Mode::Edit => {
                let len = self.buffer.chars().count();
                self.cursor = if forward {
                    (self.cursor + 1).min(len)
                } else {
                    self.cursor.saturating_sub(1)
                };
            }
            // Left/Right outside edit mode cycles dropdown options.
            _ => self.cycle_option(forward),
        }
    }

    /// Commit the edit buffer to the document.
    pub fn commit(&mut self) {
        let Some(field) = self.focused_field().cloned() else {
            self.mode = Mode::Navigate;
            return;
        };
        match self.mode {
            Mode::Edit => {
                let raw = std::mem::take(&mut self.buffer);
                let value = coerce(&raw, &field.kind);
                self.set_field_value(&field, value);
                self.saved_value = None;
            }
            Mode::Select => {
                // Already written by cycle_option.
            }
            Mode::Navigate => {}
        }
        self.mode = Mode::Navigate;
    }

    pub fn cancel(&mut self) {
        if self.mode == Mode::Edit
            && let Some(field) = self.focused_field().cloned()
        {
            let path: Vec<String> = split_data_path(&field.path);
            let original = self.saved_value.take().unwrap_or(Value::Null);
            set_nested(&mut self.data, &path, original);
            if let Some(idx) = self.field_index() {
                self.refresh_row(idx);
            }
        }
        self.buffer.clear();
        self.saved_value = None;
        self.mode = Mode::Navigate;
    }

    pub fn focus_and_edit_row(&mut self, idx: usize) {
        self.focus_row(idx);
        self.begin_edit();
    }

    pub fn get_map_entries(&self, path: &str) -> Vec<(String, String)> {
        let mut entries = Vec::new();
        if let Some(Value::Object(map)) = self.field_value(path) {
            for (k, v) in map {
                let val_str = match v {
                    Value::String(s) => s.clone(),
                    Value::Null => String::new(),
                    other => other.to_string(),
                };
                entries.push((k.clone(), val_str));
            }
        }
        entries
    }

    pub fn set_map(&mut self, path: &str, entries: Vec<(String, String)>) {
        let mut map = Map::new();
        for (k, v) in entries {
            if !k.trim().is_empty() {
                map.insert(k, Value::String(v));
            }
        }
        let segments = split_data_path(path);
        set_nested(&mut self.data, &segments, Value::Object(map));
        if let Some(idx) = self.rows.iter().position(|r| match &r.kind {
            Row::Field { field, .. } => field.path == path,
            _ => false,
        }) {
            self.refresh_row(idx);
        }
    }

    pub fn set_select_value(&mut self, path: &str, value: Option<String>) {
        let segments = split_data_path(path);
        let json_val = match value {
            Some(v) if !v.is_empty() => Value::String(v),
            _ => Value::Null,
        };
        set_nested(&mut self.data, &segments, json_val);
        if let Some(idx) = self.rows.iter().position(|r| match &r.kind {
            Row::Field { field, .. } => field.path == path,
            _ => false,
        }) {
            self.refresh_row(idx);
        }
    }

    fn toggle_boolean(&mut self) {
        let Some(field) = self.focused_field().cloned() else {
            return;
        };
        let current = self
            .field_value(&field.path)
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        self.set_field_value(&field, Value::Bool(!current));
    }

    /// Step through the focused enum/namespace options.
    fn cycle_option(&mut self, forward: bool) {
        let Some(field) = self.focused_field().cloned() else {
            return;
        };
        let options: Vec<String> = match &field.kind {
            FieldKind::Enum(vals) => vals.clone(),
            FieldKind::Namespace => self.namespaces.clone(),
            _ => return,
        };
        if options.is_empty() {
            return;
        }
        let current = self
            .field_value(&field.path)
            .and_then(|v| v.as_str())
            .map(str::to_string);
        let idx = current
            .as_deref()
            .and_then(|c| options.iter().position(|o| o == c));
        // "Blank" is the first slot, so cycling past the end clears the field.
        let next = match idx {
            None => 0,
            Some(i) if forward => (i + 1) % options.len(),
            Some(0) => options.len() - 1,
            Some(i) => i - 1,
        };
        let value = if idx.is_none() && forward {
            // entering selection from empty picks the first option
            Value::String(options[0].clone())
        } else {
            Value::String(options[next].clone())
        };
        self.set_field_value(&field, value);
    }

    // -----------------------------------------------------------------
    // Arrays
    // -----------------------------------------------------------------

    /// Materialise `n` more items in the array at `array_path`, then rebuild.
    /// Used by the `[ + add ]` button on array headers.
    pub fn add_items_at(&mut self, array_path: &str, n: usize) -> bool {
        let count = self.array_counts.entry(array_path.to_string()).or_insert(1);
        *count += n;
        self.rebuild();
        // Keep focus inside the array that just grew and ensure header is visible.
        if let Some(i) = self.rows.iter().position(|r| match &r.kind {
            Row::Array { path, .. } => path == array_path,
            _ => false,
        }) {
            let hdr_line = self.row_start_line(i);
            if hdr_line < self.scroll {
                self.scroll = hdr_line;
            }
            self.focus_next_field_from(i, true);
        }
        true
    }

    /// Materialise one more item in the array owning the focused field.
    pub fn add_array_item(&mut self) -> bool {
        let Some(field) = self.focused_field().cloned() else {
            return false;
        };
        let Some(arr_path) = array_path_of(&field.path) else {
            return false;
        };
        self.add_items_at(&arr_path, 1)
    }

    /// Remove the last item from the array at `array_path` when `count > 1`.
    pub fn remove_array_item_at(&mut self, array_path: &str) -> bool {
        let current = self
            .array_counts
            .get(array_path)
            .copied()
            .or_else(|| {
                let clean = array_path.strip_suffix("[]").unwrap_or(array_path);
                self.array_counts.get(clean).copied()
            })
            .unwrap_or(1);
        if current <= 1 {
            return false;
        }
        let new_count = current - 1;
        self.array_counts.insert(array_path.to_string(), new_count);
        let clean = array_path.strip_suffix("[]").unwrap_or(array_path);
        if self.array_counts.contains_key(clean) {
            self.array_counts.insert(clean.to_string(), new_count);
        }

        // Remove the truncated array item from `self.data` and `self.inputs`.
        let arr_norm = canonical_field_path(array_path);
        let removed_prefix = format!("{arr_norm}[{new_count}]");
        let segs: Vec<String> = clean.split('.').map(|s| s.replace("[]", "")).collect();
        truncate_array_in_value(&mut self.data, &segs, new_count);
        self.inputs.retain(|k, _| {
            !k.contains(&format!("[{new_count}]"))
                || !canonical_field_path(k).starts_with(&arr_norm)
        });
        let _ = removed_prefix;

        let prev_focus = self.focus;
        self.rebuild();
        if let Some(i) = self.rows.iter().position(|r| match &r.kind {
            Row::Array { path, .. } => path == array_path,
            _ => false,
        }) {
            let hdr_line = self.row_start_line(i);
            if hdr_line < self.scroll {
                self.scroll = hdr_line;
            }
            self.focus_next_field_from(i, true);
        } else {
            self.focus_nearest_field(prev_focus);
        }
        true
    }

    // -----------------------------------------------------------------
    // Removing Added Optional Fields & Categories
    // -----------------------------------------------------------------

    /// Returns `true` if `field_path` is an optional field added beyond the
    /// default common fields for this resource kind.
    pub fn is_added_field(&self, field_path: &str) -> bool {
        if self.extra_paths.is_empty() {
            return false;
        }
        let target = canonical_field_path(field_path);
        if target.is_empty() {
            return false;
        }
        let in_common = yamlly_core::get_common_fields_ref(&self.kind).is_some_and(|common| {
            common.iter().any(|c| {
                let cc = canonical_field_path(c);
                cc == target
                    || cc.starts_with(&format!("{target}."))
                    || target.starts_with(&format!("{cc}."))
            })
        });
        if in_common {
            return false;
        }
        self.extra_paths.iter().any(|p| {
            let cp = canonical_field_path(p);
            cp == target
                || cp.starts_with(&format!("{target}."))
                || target.starts_with(&format!("{cp}."))
        })
    }

    /// Returns `true` if the `Group` or `Array` row at `row_idx` consists
    /// entirely of added optional fields (so the entire category can be removed).
    pub fn is_added_category_at(&self, row_idx: usize) -> bool {
        if self.extra_paths.is_empty() {
            return false;
        }
        let Some(row) = self.rows.get(row_idx) else {
            return false;
        };
        let cat_depth = match &row.kind {
            Row::Group { depth, .. } | Row::Array { depth, .. } => *depth,
            _ => return false,
        };
        let mut has_fields = false;
        for r in self.rows.iter().skip(row_idx + 1) {
            if matches!(r.kind, Row::Section(_)) || r.depth <= cat_depth {
                break;
            }
            if let Row::Field { field, .. } = &r.kind {
                has_fields = true;
                if !self.is_added_field(&field.path) {
                    return false;
                }
            }
        }
        has_fields
    }

    /// Remove an added optional field at `field_path` from the form and document.
    /// Returns the removed field's short label if removed.
    pub fn remove_field(&mut self, field_path: &str) -> Option<String> {
        if !self.is_added_field(field_path) {
            return None;
        }
        let target = canonical_field_path(field_path);
        let label = self
            .rows
            .iter()
            .find_map(|r| match &r.kind {
                Row::Field { field, .. } if field.path == field_path => Some(field.label.clone()),
                _ => None,
            })
            .unwrap_or_else(|| {
                field_path
                    .rsplit('.')
                    .next()
                    .unwrap_or(field_path)
                    .replace("[]", "")
            });

        // Collect all concrete field paths matching `target` (e.g. across all array items)
        let mut concrete_paths: Vec<String> = self
            .rows
            .iter()
            .filter_map(|r| match &r.kind {
                Row::Field { field, .. } => {
                    let cf = canonical_field_path(&field.path);
                    if cf == target
                        || cf.starts_with(&format!("{target}."))
                        || target.starts_with(&format!("{cf}."))
                    {
                        Some(field.path.clone())
                    } else {
                        None
                    }
                }
                _ => None,
            })
            .collect();
        if !concrete_paths.iter().any(|p| p == field_path) {
            concrete_paths.push(field_path.to_string());
        }

        for p in &concrete_paths {
            let segs = split_data_path(p);
            unset_nested(&mut self.data, &segs);
            self.inputs.remove(p);
        }
        self.inputs.retain(|k, _| {
            let ck = canonical_field_path(k);
            !(ck == target
                || ck.starts_with(&format!("{target}."))
                || target.starts_with(&format!("{ck}.")))
        });

        self.extra_paths.retain(|p| {
            let cp = canonical_field_path(p);
            !(cp == target
                || cp.starts_with(&format!("{target}."))
                || target.starts_with(&format!("{cp}.")))
        });

        let prev_focus = self.focus;
        self.rebuild();
        self.focus_nearest_field(prev_focus);
        Some(label)
    }

    /// Remove the currently focused field if it is an added optional field.
    pub fn remove_focused_field(&mut self) -> Option<String> {
        let path = self.focused_field()?.path.clone();
        self.remove_field(&path)
    }

    /// Remove all added optional fields belonging to the category (`Group` or `Array`) at `row_idx`.
    /// Returns `(category_label, removed_field_count)` if removed.
    pub fn remove_category_at(&mut self, row_idx: usize) -> Option<(String, usize)> {
        if !self.is_added_category_at(row_idx) {
            return None;
        }
        let (cat_label, cat_depth, arr_path_opt) = match &self.rows.get(row_idx)?.kind {
            Row::Group { label, depth } => (label.clone(), *depth, None),
            Row::Array {
                key, path, depth, ..
            } => (key.clone(), *depth, Some(path.clone())),
            _ => return None,
        };

        let mut concrete_paths = Vec::new();
        let mut canonical_targets = Vec::new();
        for r in self.rows.iter().skip(row_idx + 1) {
            if matches!(r.kind, Row::Section(_)) || r.depth <= cat_depth {
                break;
            }
            if let Row::Field { field, .. } = &r.kind {
                concrete_paths.push(field.path.clone());
                let cf = canonical_field_path(&field.path);
                if !canonical_targets.contains(&cf) {
                    canonical_targets.push(cf);
                }
            }
        }
        if canonical_targets.is_empty() {
            return None;
        }

        for p in &concrete_paths {
            let segs = split_data_path(p);
            unset_nested(&mut self.data, &segs);
            self.inputs.remove(p);
        }
        for target in &canonical_targets {
            self.inputs.retain(|k, _| {
                let ck = canonical_field_path(k);
                !(ck == *target
                    || ck.starts_with(&format!("{target}."))
                    || target.starts_with(&format!("{ck}.")))
            });
            self.extra_paths.retain(|p| {
                let cp = canonical_field_path(p);
                !(cp == *target
                    || cp.starts_with(&format!("{target}."))
                    || target.starts_with(&format!("{cp}.")))
            });
        }
        if let Some(arr_path) = arr_path_opt {
            self.array_counts.remove(&arr_path);
            let clean = arr_path.strip_suffix("[]").unwrap_or(&arr_path);
            self.array_counts.remove(clean);
            let segs: Vec<String> = clean.split('.').map(|s| s.replace("[]", "")).collect();
            unset_nested(&mut self.data, &segs);
        }

        let removed_count = canonical_targets.len();
        self.rebuild();
        self.focus_nearest_field(row_idx);
        Some((cat_label, removed_count))
    }

    /// Move `self.focus` to the nearest visible `Row::Field` around `preferred_idx`.
    pub fn focus_nearest_field(&mut self, preferred_idx: usize) {
        if self.rows.is_empty() {
            self.focus = 0;
            return;
        }
        let vis = self.row_visibility();
        let clamped = preferred_idx.min(self.rows.len() - 1);
        if vis.get(clamped).copied().unwrap_or(true)
            && matches!(self.rows[clamped].kind, Row::Field { .. })
        {
            self.focus = clamped;
            return;
        }
        // Search backward first so removing a field focuses the field right above it (or next below)
        for i in (0..=clamped).rev() {
            if vis.get(i).copied().unwrap_or(true) && matches!(self.rows[i].kind, Row::Field { .. })
            {
                self.focus = i;
                return;
            }
        }
        for i in (clamped + 1)..self.rows.len() {
            if vis.get(i).copied().unwrap_or(true) && matches!(self.rows[i].kind, Row::Field { .. })
            {
                self.focus = i;
                return;
            }
        }
    }

    // -----------------------------------------------------------------
    // Validation
    // -----------------------------------------------------------------

    /// Validate against the schema, annotate the offending rows, and return
    /// the error count.
    ///
    /// Two checks, mirroring the original app: required fields that are empty,
    /// and integer-typed fields holding non-numeric text (which `coerce`
    /// deliberately preserves so the user can see what they typed).
    pub fn validate(&mut self) -> usize {
        let errors = self.collect_errors();
        let count = errors.len();

        // Clear stale markers, then re-annotate.
        for row in &mut self.rows {
            row.error = None;
        }
        for (path, msg) in &errors {
            for row in &mut self.rows {
                if let Row::Field { field, .. } = &row.kind
                    && &field.path == path
                {
                    row.error = Some(msg.clone());
                    break;
                }
            }
        }
        count
    }

    /// Paths + messages for every validation failure.
    ///
    /// Only *rendered* fields are checked. The generated schema marks nothing
    /// at the top level as required, so walking the document top-down would
    /// stop at an empty `spec` and report a completely blank form as valid.
    /// Checking what the user can actually see is both more useful and honest
    /// about what this phase validates.
    pub fn collect_errors(&self) -> Vec<(String, String)> {
        let mut errors = Vec::new();

        for row in &self.rows {
            let Row::Field { field, .. } = &row.kind else {
                continue;
            };
            let value = self.field_value(&field.path);

            // 1. Required but empty.
            if field.required && value.is_none_or(is_blank) {
                errors.push((field.path.clone(), "Field is required.".into()));
                continue;
            }
            // 2. Integer field holding something that is not a number.
            if matches!(field.kind, FieldKind::Integer) {
                let bad = match value {
                    None | Some(Value::Null) => false,
                    Some(Value::Number(_)) => false,
                    Some(Value::String(s)) => {
                        let t = s.trim();
                        !t.is_empty() && t.parse::<i64>().is_err()
                    }
                    Some(_) => true,
                };
                if bad {
                    errors.push((field.path.clone(), "must be a number".into()));
                }
            }
        }
        errors
    }

    // -----------------------------------------------------------------
    // Layout metrics
    // -----------------------------------------------------------------

    /// Screen lines each logical row occupies. Fields render as a bordered
    /// box (label tab + value + bottom border); headers are a single line.
    /// Virtual height of each row.
    ///
    /// A field is two lines (label, then value). A section rule is one line,
    /// except after the first section, where it is preceded by a blank line so
    /// the form reads as sections rather than one continuous block.
    ///
    /// This is the single source of truth for the form's line space: the
    /// renderer, scrolling, focus-follow and hit-testing all derive from it, so
    /// a blank line added here cannot desynchronise them.
    pub fn block_heights(&self) -> Vec<usize> {
        let vis = self.row_visibility();
        let mut seen_section = false;
        self.rows
            .iter()
            .enumerate()
            .map(|(i, r)| {
                if !vis.get(i).copied().unwrap_or(true) {
                    return 0;
                }
                match r.kind {
                    Row::Field { .. } => 4,
                    Row::Array { .. } => 4,
                    Row::Section(_) if std::mem::replace(&mut seen_section, true) => 3,
                    Row::Section(_) => 2,
                    Row::Group { .. } => 2,
                }
            })
            .collect()
    }

    /// Virtual line span `[start, end)` of the focused row.
    pub fn focus_lines(&self) -> (usize, usize) {
        let heights = self.block_heights();
        let mut start = 0usize;
        for (i, h) in heights.iter().enumerate() {
            if i == self.focus {
                return (start, start + h);
            }
            start += h;
        }
        (0, 1)
    }

    /// Virtual line count of the whole form.
    pub fn total_lines(&self) -> usize {
        self.block_heights().iter().sum()
    }

    /// First virtual line index of logical row `idx`.
    pub fn row_start_line(&self, idx: usize) -> usize {
        self.block_heights().iter().take(idx).sum()
    }

    // -----------------------------------------------------------------
    // Dirty tracking
    // -----------------------------------------------------------------

    pub fn is_dirty(&self) -> bool {
        yamlly_core::is_dirty(&self.data, &self.baseline)
    }

    pub fn mark_clean(&mut self) {
        self.baseline = normalize(&self.data).unwrap_or(Value::Object(Map::new()));
    }
}

/// Treat null, empty string, and empty containers as "no value entered".
fn is_blank(v: &Value) -> bool {
    match v {
        Value::Null => true,
        Value::String(s) => s.is_empty(),
        Value::Array(a) => a.is_empty(),
        Value::Object(o) => o.is_empty(),
        _ => false,
    }
}

/// Split a dotted data path (with `[N]`) into the segments `set_nested` wants.
pub fn split_data_path(path: &str) -> Vec<String> {
    let mut out = Vec::new();
    for part in path.split('.') {
        let name: String = part.chars().take_while(|c| *c != '[').collect();
        let rest = &part[name.len()..];
        if !name.is_empty() {
            out.push(name);
        }
        for idx in rest.split('[').skip(1) {
            let digits: String = idx.chars().take_while(|c| c.is_ascii_digit()).collect();
            if !digits.is_empty() {
                out.push(digits);
            }
        }
    }
    out
}

/// The `...foo[]` path of the array containing `field_path`.
///
/// The result keeps the `[]` suffix so it matches the key `build_form` uses in
/// `array_counts`.
fn array_path_of(field_path: &str) -> Option<String> {
    let mut acc = String::new();
    for part in field_path.split('.') {
        let name: String = part.chars().take_while(|c| *c != '[').collect();
        if !acc.is_empty() {
            acc.push('.');
        }
        acc.push_str(&name);
        if part.contains('[') {
            acc.push_str("[]");
            return Some(acc);
        }
    }
    None
}

fn char_to_byte(s: &str, char_idx: usize) -> usize {
    s.char_indices()
        .nth(char_idx)
        .map(|(b, _)| b)
        .unwrap_or(s.len())
}

fn value_to_display(v: &Value) -> String {
    match v {
        Value::String(s) => s.clone(),
        Value::Null => String::new(),
        other => other.to_string(),
    }
}

/// Turn edit-buffer text into the right JSON type for the field.
fn coerce(raw: &str, kind: &FieldKind) -> Value {
    if raw.is_empty() {
        return Value::Null;
    }
    match kind {
        FieldKind::Integer => match raw.parse::<i64>() {
            Ok(n) => json!(n),
            // Keep the text so a partial entry is not silently destroyed;
            // validation flags it.
            Err(_) => json!(raw),
        },
        FieldKind::Boolean => json!(raw == "true"),
        FieldKind::StringList => {
            let items: Vec<Value> = raw
                .split(',')
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(|s| Value::String(s.to_string()))
                .collect();
            if items.is_empty() {
                Value::Null
            } else {
                Value::Array(items)
            }
        }
        _ => json!(raw),
    }
}

/// Recursively collect dotted schema paths and array item counts from a data Value.
fn collect_data_paths(
    val: &Value,
    prefix: &str,
    schema_fields: Option<&Value>,
    paths: &mut Vec<String>,
    array_counts: &mut HashMap<String, usize>,
) {
    let push_unique = |paths: &mut Vec<String>, p: String| {
        if !paths.contains(&p) {
            paths.push(p);
        }
    };
    match val {
        Value::Object(map) => {
            for (k, v) in map {
                if prefix.is_empty() && (k == "apiVersion" || k == "kind") {
                    continue;
                }
                let p = if prefix.is_empty() {
                    k.clone()
                } else {
                    format!("{prefix}.{k}")
                };
                let has_schema_children = match schema_fields {
                    Some(sf) => {
                        let lookup = yamlly_core::field::schema_lookup_path(&p);
                        yamlly_core::resolve_field_path_ref(sf, &lookup)
                            .and_then(|n| n.get("fields"))
                            .and_then(|f| f.as_object())
                            .is_some_and(|m| !m.is_empty())
                    }
                    None => match v {
                        Value::Object(_) => true,
                        Value::Array(arr) => arr.iter().any(Value::is_object),
                        _ => false,
                    },
                };
                if !has_schema_children {
                    if !v.is_null() {
                        push_unique(paths, p);
                    }
                    continue;
                }
                match v {
                    Value::Object(inner) if !inner.is_empty() => {
                        collect_data_paths(v, &p, schema_fields, paths, array_counts);
                    }
                    Value::Array(arr) => {
                        let count = arr.len();
                        if count > 0 {
                            let clean_p = p.strip_suffix("[]").unwrap_or(&p);
                            let arr_key = format!("{clean_p}[]");
                            let entry = array_counts.entry(arr_key.clone()).or_insert(0);
                            *entry = (*entry).max(count);
                            for item in arr {
                                collect_data_paths(
                                    item,
                                    &arr_key,
                                    schema_fields,
                                    paths,
                                    array_counts,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        Value::Array(arr) => {
            let count = arr.len();
            if count > 0 {
                let clean_p = prefix.strip_suffix("[]").unwrap_or(prefix);
                let arr_key = format!("{clean_p}[]");
                let entry = array_counts.entry(arr_key.clone()).or_insert(0);
                *entry = (*entry).max(count);
                for item in arr {
                    collect_data_paths(item, &arr_key, schema_fields, paths, array_counts);
                }
            } else if !prefix.is_empty() {
                push_unique(paths, prefix.to_string());
            }
        }
        _ => {
            if !prefix.is_empty() {
                push_unique(paths, prefix.to_string());
            }
        }
    }
}

/// Normalize concrete and schema paths by mapping index brackets `[0]` -> `[]`.
fn normalize_for_match(path: &str) -> String {
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
        let ch = path[i..].chars().next().unwrap_or('\0');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Normalize concrete and schema paths by mapping `[0]` -> `[]` and stripping any trailing `[]`.
fn canonical_field_path(path: &str) -> String {
    let norm = normalize_for_match(path);
    norm.strip_suffix("[]").unwrap_or(&norm).to_string()
}

/// Remove a nested leaf value at `segments` and prune empty parent objects.
fn unset_nested(node: &mut Value, segments: &[String]) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    match node {
        Value::Object(map) => {
            if rest.is_empty() {
                map.remove(first.as_str());
            } else if let Some(child) = map.get_mut(first.as_str()) {
                unset_nested(child, rest);
                let child_empty = match child {
                    Value::Null => true,
                    Value::Object(m) => m.is_empty(),
                    Value::Array(a) => a.is_empty(),
                    _ => false,
                };
                if child_empty {
                    map.remove(first.as_str());
                }
            }
        }
        Value::Array(arr) => {
            if let Ok(idx) = first.parse::<usize>() {
                if rest.is_empty() {
                    if idx < arr.len() {
                        arr.remove(idx);
                    }
                } else if let Some(child) = arr.get_mut(idx) {
                    unset_nested(child, rest);
                }
            }
        }
        _ => {}
    }
}

/// Truncate an array located at `segments` (walking through any intermediate arrays/objects) to `new_len`.
fn truncate_array_in_value(node: &mut Value, segments: &[String], new_len: usize) {
    let Some((first, rest)) = segments.split_first() else {
        return;
    };
    match node {
        Value::Object(map) => {
            if let Some(child) = map.get_mut(first.as_str()) {
                if rest.is_empty() {
                    if let Value::Array(arr) = child {
                        arr.truncate(new_len);
                    }
                } else {
                    truncate_array_in_value(child, rest, new_len);
                }
            }
        }
        Value::Array(arr) => {
            for item in arr.iter_mut() {
                truncate_array_in_value(item, segments, new_len);
            }
        }
        _ => {}
    }
}
