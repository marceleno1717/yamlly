//! Schema-driven form model.
//!
//! Turns a resource kind plus its schema into an ordered list of render rows,
//! and classifies each leaf into a [`FieldKind`] that tells the UI which input
//! primitive to draw. This module is deliberately free of any UI dependency so
//! the layout logic stays unit-testable; `yamlly-cli` owns the interaction.

use serde_json::Value;
use std::collections::HashMap;

use crate::loader::resolve_field_path_ref;

/// Which input primitive renders a given leaf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldKind {
    Text,
    Integer,
    Boolean,
    /// Fixed option list, cycled with Left/Right.
    Enum(Vec<String>),
    /// Populated from the live cluster namespace list.
    Namespace,
    /// Key/value rows (labels, annotations, resource requests).
    Map,
    /// `[]string` rendered comma-separated, matching the original app.
    StringList,
}

impl FieldKind {
    /// Short label for the input primitive, used in diagnostics and tests.
    pub fn name(&self) -> &'static str {
        match self {
            FieldKind::Text => "text",
            FieldKind::Integer => "integer",
            FieldKind::Boolean => "boolean",
            FieldKind::Enum(_) => "enum",
            FieldKind::Namespace => "namespace",
            FieldKind::Map => "map",
            FieldKind::StringList => "string-list",
        }
    }

    /// True when the value round-trips through free-text editing.
    pub fn is_textual(&self) -> bool {
        matches!(
            self,
            FieldKind::Text
                | FieldKind::Integer
                | FieldKind::StringList
                | FieldKind::Map
                | FieldKind::Enum(_)
                | FieldKind::Namespace
        )
    }
}

/// Enum values for paths where the schema does not carry an inline enum.
const KNOWN_ENUMS: &[(&str, &[&str])] = &[
    (
        "spec.type",
        &["ClusterIP", "NodePort", "LoadBalancer", "ExternalName"],
    ),
    ("spec.restartPolicy", &["Always", "OnFailure", "Never"]),
    (
        "spec.accessModes",
        &[
            "ReadWriteOnce",
            "ReadOnlyMany",
            "ReadWriteMany",
            "ReadWriteOncePod",
        ],
    ),
    ("spec.volumeMode", &["Filesystem", "Block"]),
    (
        "spec.persistentVolumeReclaimPolicy",
        &["Retain", "Recycle", "Delete"],
    ),
    ("spec.updateStrategy.type", &["RollingUpdate", "OnDelete"]),
    (
        "spec.rules[].http.paths[].pathType",
        &["Prefix", "Exact", "ImplementationSpecific"],
    ),
    (
        "spec.metrics[].type",
        &[
            "Resource",
            "Pods",
            "Object",
            "External",
            "ContainerResource",
        ],
    ),
    (
        "spec.metrics[].resource.target.type",
        &["Utilization", "Value", "AverageValue"],
    ),
    ("spec.policyTypes", &["Ingress", "Egress"]),
];

const NAMESPACE_PATHS: &[&str] = &["metadata.namespace", "spec.template.metadata.namespace"];

/// Strip `[N]` / `[]` so a concrete data path can be resolved against the
/// schema tree, which is indexed without array suffixes.
pub fn schema_lookup_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    let bytes = path.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'[' {
            // Only strip a *complete* `[...]` group. An unterminated `[` is
            // literal data, matching the original's `re.sub(r"\[\d*\]", ...)`.
            match path[i..].find(']') {
                Some(end) => {
                    i += end + 1;
                    continue;
                }
                None => {
                    out.push('[');
                    i += 1;
                    continue;
                }
            }
        }
        // Copy one whole char so multi-byte input is preserved.
        let ch = path[i..].chars().next().unwrap_or('\u{fffd}');
        out.push(ch);
        i += ch.len_utf8();
    }
    out
}

/// Decide which primitive renders `path` given its schema `ftype`.
pub fn classify(path: &str, ftype: &str, namespaces: &[String]) -> FieldKind {
    let clean = schema_lookup_path(path);

    if namespaces.len() > 1 && NAMESPACE_PATHS.contains(&clean.as_str()) {
        return FieldKind::Namespace;
    }
    // Suffix match: the table is written in terms of the Pod-level path
    // (`spec.restartPolicy`), but a Deployment carries the same field nested
    // under `spec.template.spec`. The longest matching suffix wins so the
    // more specific `spec.metrics[].type` beats a generic prefix.
    let table: &[(&str, &[&str])] = KNOWN_ENUMS;
    let best = table
        .iter()
        .filter(|(k, _)| *k == clean || *k == path)
        .chain(
            table
                .iter()
                .filter(|(k, _)| clean.ends_with(&format!(".{}", k))),
        )
        .max_by_key(|(k, _)| k.len());
    if let Some((_, vals)) = best {
        return FieldKind::Enum(vals.iter().map(|s| (*s).to_string()).collect());
    }

    match ftype {
        "boolean" | "bool" => FieldKind::Boolean,
        "integer" | "int" => FieldKind::Integer,
        // The original treats any `map[...]` container, plus bare `object`,
        // as a key/value editor. `map[string]Quantity` included.
        t if t == "map[string]string" || t == "object" || t.contains("map[") => FieldKind::Map,
        t if t.starts_with("[]") || t == "array" => FieldKind::StringList,
        _ => FieldKind::Text,
    }
}

impl Row {
    /// Layout depth, used for indentation. Sections are always at the top.
    pub fn depth(&self) -> usize {
        match self {
            Row::Section(_) => 0,
            Row::Group { depth, .. } | Row::Array { depth, .. } => *depth,
            Row::Field { depth, .. } => *depth,
        }
    }
}

/// One editable leaf of the form.
#[derive(Debug, Clone, PartialEq)]
pub struct FormField {
    /// Concrete data path, including any `[N]` indices (`...containers[0].image`).
    pub path: String,
    /// Leaf name shown next to the input.
    pub label: String,
    pub kind: FieldKind,
    pub required: bool,
}

/// A single line of the rendered form.
#[derive(Debug, Clone, PartialEq)]
pub enum Row {
    /// Top-level grouping (`metadata`, `spec`).
    Section(String),
    /// Nested grouping header (`template`, `resources`).
    Group {
        label: String,
        depth: usize,
    },
    /// `containers[]` with how many items are currently materialised.
    Array {
        key: String,
        /// Dotted path of the array itself, with the `[]` suffix.
        path: String,
        depth: usize,
        count: usize,
    },
    Field {
        field: FormField,
        depth: usize,
    },
}

/// Ordered path tree, mirroring the original app's `_build_path_tree`.
#[derive(Debug, Clone, Default)]
pub struct PathTree {
    pub entries: Vec<(String, Entry)>,
}

#[derive(Debug, Clone)]
pub enum Entry {
    Branch(PathTree),
    /// Full dotted path of the leaf.
    Leaf(String),
}

/// Build the nested tree for a section's dotted paths. The first path segment
/// is the section title and is not represented as a node, matching the
/// original's behaviour.
pub fn build_path_tree(paths: &[String]) -> PathTree {
    let mut tree = PathTree::default();
    for p in paths {
        let parts: Vec<&str> = p.split('.').collect();
        // The first segment is the section title, not a node.
        if parts.len() == 1 {
            set_leaf(&mut tree, parts[0], p);
        } else {
            insert_into(&mut tree, &parts[1..], p);
        }
    }
    tree
}

/// Recursively place `dotted` at `parts`, creating intermediate branches.
///
/// `parts` always includes the leaf as its last element; only the leading
/// element is consumed per step. Descending on `parts[1..]` (not
/// `parents[1..]`) is what keeps a trailing array segment like
/// `containers[].env[]` a *branch* rather than overwriting it as a leaf.
fn insert_into(node: &mut PathTree, parts: &[&str], dotted: &str) {
    let Some((last, parents)) = parts.split_last() else {
        return;
    };
    if parents.is_empty() {
        set_leaf(node, last, dotted);
        return;
    }
    let Some(idx) = ensure_branch(node, parents[0]) else {
        return;
    };
    if let Some(child) = branch_mut(node, idx) {
        insert_into(child, &parts[1..], dotted);
    }
}

fn set_leaf(tree: &mut PathTree, key: &str, dotted: &str) {
    let clean = clean_key(key);
    if tree
        .entries
        .iter()
        .any(|(k, e)| clean_key(k) == clean && matches!(e, Entry::Branch(_)))
    {
        // Do not overwrite an existing category branch with a leaf.
        return;
    }
    match tree.entries.iter_mut().find(|(k, _)| clean_key(k) == clean) {
        Some((_, e)) => *e = Entry::Leaf(dotted.to_string()),
        None => tree
            .entries
            .push((key.to_string(), Entry::Leaf(dotted.to_string()))),
    }
}

/// Index of the branch under `key`, creating one if absent. `None` when the
/// key exists but holds a leaf (a path-shape conflict; the insert is dropped).
fn ensure_branch(tree: &mut PathTree, key: &str) -> Option<usize> {
    let clean = clean_key(key);
    let idx = match tree.entries.iter().position(|(k, _)| clean_key(k) == clean) {
        Some(i) => {
            if key.ends_with("[]") && !tree.entries[i].0.ends_with("[]") {
                tree.entries[i].0 = key.to_string();
            }
            i
        }
        None => {
            tree.entries
                .push((key.to_string(), Entry::Branch(PathTree::default())));
            tree.entries.len() - 1
        }
    };
    matches!(tree.entries[idx].1, Entry::Branch(_)).then_some(idx)
}

fn branch_mut(tree: &mut PathTree, idx: usize) -> Option<&mut PathTree> {
    match &mut tree.entries[idx].1 {
        Entry::Branch(b) => Some(b),
        Entry::Leaf(_) => None,
    }
}

fn clean_key(key: &str) -> String {
    key.replace("[]", "").replace("-ARR", "")
}

/// Decide whether a field is required in forms and validation.
///
/// Port numbers (`containerPort`, `port`, `targetPort`, `hostPort`, etc.) and environment
/// variable definitions (`env`, `envFrom`) are not required in Kubernetes manifests even if
/// the underlying OpenAPI schema marks child fields as required when their parent
/// object is present.
pub fn is_field_required(path: &str, schema_required: bool) -> bool {
    if !schema_required {
        return false;
    }
    let lookup = schema_lookup_path(path);
    for seg in lookup.split('.') {
        if seg == "containerPort"
            || seg == "port"
            || seg == "targetPort"
            || seg == "hostPort"
            || seg == "env"
            || seg == "envFrom"
        {
            return false;
        }
    }
    true
}

/// Flatten the path tree into renderable [`Row`]s.
///
/// `array_counts` lets the caller materialise N items per array (the original
/// auto-adds item 0 on mount); unknown arrays default to 1. `replacements`
/// substitutes `containers[]` -> `containers[0]` so nested leaves point at real
/// data paths.
/// Walk context threaded through [`render_tree`].
struct RenderCtx<'a> {
    schema_fields: &'a Value,
    namespaces: &'a [String],
    array_counts: &'a HashMap<String, usize>,
    replacements: Vec<(String, String)>,
    current_path: String,
    depth: usize,
}

fn render_tree(node: &PathTree, ctx: &mut RenderCtx, out: &mut Vec<Row>) {
    // Pass 1: Render direct leaf fields belonging directly to this category level first.
    for (_key, entry) in &node.entries {
        if let Entry::Leaf(full_path) = entry {
            let mut path = full_path
                .strip_suffix("[]")
                .unwrap_or(full_path)
                .to_string();
            for (from, to) in &ctx.replacements {
                path = path.replace(from.as_str(), to.as_str());
            }
            let lookup = schema_lookup_path(&path);
            let node_ref = resolve_field_path_ref(ctx.schema_fields, &lookup);
            let ftype = node_ref
                .and_then(|n| n.get("type"))
                .and_then(|v| v.as_str())
                .unwrap_or("string");
            let raw_req = node_ref
                .and_then(|n| n.get("required"))
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let required = is_field_required(&path, raw_req);
            out.push(Row::Field {
                field: FormField {
                    label: clean_key(path.rsplit('.').next().unwrap_or(&path)),
                    kind: classify(&path, ftype, ctx.namespaces),
                    required,
                    path,
                },
                depth: ctx.depth,
            });
        }
    }

    // Pass 2: Render nested sub-branches and sub-arrays.
    for (key, entry) in &node.entries {
        if let Entry::Branch(children) = entry {
            let node_path = if ctx.current_path.is_empty() {
                key.clone()
            } else {
                format!("{}.{}", ctx.current_path, key)
            };

            let is_array = key.ends_with("[]") || key.ends_with("-ARR");
            let (saved_repl, saved_path, saved_depth) = (
                ctx.replacements.clone(),
                ctx.current_path.clone(),
                ctx.depth,
            );
            ctx.current_path = node_path.clone();
            ctx.depth = saved_depth + 1;

            if is_array {
                // Every materialised index gets its own pass so each
                // `containers[N].*` leaf resolves to a distinct data path.
                let count = ctx
                    .array_counts
                    .get(&node_path)
                    .copied()
                    .unwrap_or(1)
                    .max(1);
                out.push(Row::Array {
                    key: clean_key(key),
                    path: node_path.clone(),
                    depth: saved_depth,
                    count,
                });
                for i in 0..count {
                    ctx.replacements = saved_repl.clone();
                    ctx.replacements
                        .push((key.clone(), format!("{}[{}]", clean_key(key), i)));
                    render_tree(children, ctx, out);
                }
            } else {
                out.push(Row::Group {
                    label: clean_key(key),
                    depth: saved_depth,
                });
                ctx.replacements = saved_repl.clone();
                render_tree(children, ctx, out);
            }

            ctx.replacements = saved_repl;
            ctx.current_path = saved_path;
            ctx.depth = saved_depth;
        }
    }
}

/// Build the full form for `kind`.
///
/// Returns the render rows plus the set of paths already shown (the field
/// picker uses this to filter). `array_counts` is threaded through untouched so
/// the UI can persist item counts across rebuilds.
pub fn build_form(
    kind: &str,
    namespaces: &[String],
    extra_paths: &[String],
    array_counts: &HashMap<String, usize>,
) -> Option<(Vec<Row>, Vec<String>)> {
    build_form_with_mode(kind, namespaces, extra_paths, array_counts, false)
}

/// Build the form for `kind`, either with full `common_fields` (`express_mode = false`)
/// or minimal `express_fields` (`express_mode = true`).
pub fn build_form_with_mode(
    kind: &str,
    namespaces: &[String],
    extra_paths: &[String],
    array_counts: &HashMap<String, usize>,
    express_mode: bool,
) -> Option<(Vec<Row>, Vec<String>)> {
    let resource = crate::loader::load_resource_ref(kind)?;
    let schema_fields = resource.get("fields")?;

    let cached = if express_mode {
        crate::loader::express_fields_cached(kind).unwrap_or_default()
    } else {
        crate::loader::common_fields_cached(kind).unwrap_or_default()
    };
    let mut common: Vec<String> = cached.to_vec();
    // Extra paths (from the field picker or an opened file) extend the default
    // set without duplicates or singleton category conflicts.
    for p in extra_paths {
        let norm_p = p.strip_suffix("[]").unwrap_or(p);
        let already = common.iter().any(|c| {
            let norm_c = c.strip_suffix("[]").unwrap_or(c);
            norm_c == norm_p
                || norm_c.starts_with(&format!("{norm_p}."))
                || norm_p.starts_with(&format!("{norm_c}."))
        });
        if !already {
            common.push(p.clone());
        }
    }

    let shown: Vec<String> = common.clone();
    let mut rows = Vec::new();

    for (section_name, sec_paths) in group_sections(&common) {
        let tree = build_path_tree(&sec_paths);
        rows.push(Row::Section(section_name.clone()));
        let mut ctx = RenderCtx {
            schema_fields,
            namespaces,
            array_counts,
            replacements: Vec::new(),
            current_path: section_name,
            depth: 1,
        };
        render_tree(&tree, &mut ctx, &mut rows);
    }
    Some((rows, shown))
}

/// Group dotted paths by their top-level key, preserving first-seen order.
pub fn group_sections(paths: &[String]) -> Vec<(String, Vec<String>)> {
    let mut sections: Vec<(String, Vec<String>)> = Vec::new();
    for path in paths {
        let top = path.split('.').next().unwrap_or("").to_string();
        match sections.iter_mut().find(|(s, _)| *s == top) {
            Some((_, v)) => v.push(path.clone()),
            None => sections.push((top, vec![path.clone()])),
        }
    }
    sections
}
