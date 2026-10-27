use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::LazyLock;

const INDEX_JSON: &str = include_str!("../schemas/index.json");

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ResourceIndexEntry {
    pub kind: String,
    #[serde(rename = "apiVersion")]
    pub api_version: String,
    #[serde(default)]
    pub resource: Option<String>,
    #[serde(default)]
    pub common_fields: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct IndexContainer {
    #[serde(default)]
    resources: Vec<ResourceIndexEntry>,
}

static INDEX_ENTRIES: LazyLock<Vec<ResourceIndexEntry>> = LazyLock::new(|| {
    let container: IndexContainer = serde_json::from_str(INDEX_JSON).unwrap_or(IndexContainer {
        resources: Vec::new(),
    });
    let mut list = container.resources;
    list.sort_by(|a, b| a.kind.cmp(&b.kind));
    list
});

static SHARDS: LazyLock<HashMap<&'static str, Value>> = LazyLock::new(|| {
    let mut m: HashMap<&'static str, Value> = HashMap::with_capacity(22);
    let insert = |m: &mut HashMap<&'static str, Value>, kind: &'static str, raw: &'static str| {
        if let Ok(v) = serde_json::from_str::<Value>(raw) {
            m.insert(kind, v);
        }
    };
    insert(
        &mut m,
        "clusterrole",
        include_str!("../schemas/clusterrole.json"),
    );
    insert(
        &mut m,
        "clusterrolebinding",
        include_str!("../schemas/clusterrolebinding.json"),
    );
    insert(
        &mut m,
        "configmap",
        include_str!("../schemas/configmap.json"),
    );
    insert(&mut m, "cronjob", include_str!("../schemas/cronjob.json"));
    insert(
        &mut m,
        "daemonset",
        include_str!("../schemas/daemonset.json"),
    );
    insert(
        &mut m,
        "deployment",
        include_str!("../schemas/deployment.json"),
    );
    insert(
        &mut m,
        "horizontalpodautoscaler",
        include_str!("../schemas/horizontalpodautoscaler.json"),
    );
    insert(&mut m, "ingress", include_str!("../schemas/ingress.json"));
    insert(&mut m, "job", include_str!("../schemas/job.json"));
    insert(
        &mut m,
        "limitrange",
        include_str!("../schemas/limitrange.json"),
    );
    insert(
        &mut m,
        "namespace",
        include_str!("../schemas/namespace.json"),
    );
    insert(
        &mut m,
        "networkpolicy",
        include_str!("../schemas/networkpolicy.json"),
    );
    insert(
        &mut m,
        "persistentvolume",
        include_str!("../schemas/persistentvolume.json"),
    );
    insert(
        &mut m,
        "persistentvolumeclaim",
        include_str!("../schemas/persistentvolumeclaim.json"),
    );
    insert(&mut m, "pod", include_str!("../schemas/pod.json"));
    insert(
        &mut m,
        "resourcequota",
        include_str!("../schemas/resourcequota.json"),
    );
    insert(&mut m, "role", include_str!("../schemas/role.json"));
    insert(
        &mut m,
        "rolebinding",
        include_str!("../schemas/rolebinding.json"),
    );
    insert(&mut m, "secret", include_str!("../schemas/secret.json"));
    insert(&mut m, "service", include_str!("../schemas/service.json"));
    insert(
        &mut m,
        "serviceaccount",
        include_str!("../schemas/serviceaccount.json"),
    );
    insert(
        &mut m,
        "statefulset",
        include_str!("../schemas/statefulset.json"),
    );
    m
});

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LeafFieldPath {
    pub path: String,
    pub field_type: String,
    pub required: bool,
}

pub fn get_all_resources_ref() -> &'static [ResourceIndexEntry] {
    &INDEX_ENTRIES
}

pub fn get_all_resources() -> Vec<ResourceIndexEntry> {
    INDEX_ENTRIES.clone()
}

pub fn get_common_fields_ref(kind: &str) -> Option<&'static [String]> {
    // Index rather than `find`: the closure would otherwise tie the returned
    // reference's lifetime to `kind` instead of the `'static` entry list.
    INDEX_ENTRIES
        .iter()
        .position(|e| e.kind.eq_ignore_ascii_case(kind))
        .map(|i| INDEX_ENTRIES[i].common_fields.as_slice())
}

pub fn get_common_fields(kind: &str) -> Vec<String> {
    get_common_fields_ref(kind)
        .map(<[String]>::to_vec)
        .unwrap_or_default()
}

pub fn group_common_fields_by_section(common_paths: &[String]) -> IndexMap<String, Vec<String>> {
    let mut sections: IndexMap<String, Vec<String>> = IndexMap::new();
    for path in common_paths {
        let top = path.split('.').next().unwrap_or("").to_string();
        sections.entry(top).or_default().push(path.clone());
    }
    sections
}

/// Deep-clones a shard. Convenient, but a Pod schema is ~270KB of JSON and
/// the clone costs ~0.8ms plus the matching free -- call it from tests and
/// one-shot tooling only. For anything on an interaction path, prefer
/// [`load_resource_ref`], which hands back the already-parsed static value.
pub fn load_resource(kind: &str) -> Option<Value> {
    let key = kind.to_ascii_lowercase();
    SHARDS.get(key.as_str()).cloned()
}

pub fn load_resource_ref(kind: &str) -> Option<&'static Value> {
    // lowercase lookup without extra alloc beyond the one for key
    let lower = kind.to_ascii_lowercase();
    SHARDS.get(lower.as_str())
}

pub fn get_leaf_field_paths(fields: &Value) -> Vec<LeafFieldPath> {
    let mut results = Vec::new();
    if let Value::Object(map) = fields {
        _walk_leaf_paths(map, "", &mut results);
    }
    results
}

/// Cached `common_fields` per kind, sorted for a stable binary search.
///
/// `build_form` looks this up on every rebuild; the schema never changes at
/// runtime, so the clone-and-sort would be pure repeat work.
/// `kind` is matched case-insensitively (`Pod` and `pod` both resolve), but
/// the index stores canonical casing (`Pod`), so lowercase *before* caching
/// the key and compare case-insensitively.
pub fn common_fields_cached(kind: &str) -> Option<&'static [String]> {
    // `&'static` via a one-shot leak: bounded by the 22 known kinds, each
    // computed at most once. Same pattern as `get_leaf_field_paths_cached`.
    static CACHE: LazyLock<std::sync::RwLock<HashMap<String, &'static [String]>>> =
        LazyLock::new(|| std::sync::RwLock::new(HashMap::new()));
    let key = kind.to_ascii_lowercase();
    if let Ok(g) = CACHE.read() {
        if let Some(hit) = g.get(&key) {
            return Some(hit);
        }
    }
    let entry = get_common_fields_ref(&key)?;
    let leaked: &'static [String] = Box::leak(entry.to_vec().into_boxed_slice());
    let mut g = CACHE.write().ok()?;
    Some(*g.entry(key).or_insert(leaked))
}

/// Cached variant for a loaded shard. The schema is immutable once parsed, so
/// walking ~800 fields on every call is pure waste; this memoises per kind.
/// Falls back to `None` if `kind` is not a known shard.
///
/// Bounded by design: only the 22 known shards can ever be cached, and each is
/// computed at most once, so the permanently-retained memory is a one-off.
pub fn get_leaf_field_paths_cached(kind: &str) -> Option<&'static [LeafFieldPath]> {
    static CACHE: LazyLock<std::sync::RwLock<HashMap<String, &'static [LeafFieldPath]>>> =
        LazyLock::new(|| std::sync::RwLock::new(HashMap::new()));
    let key = kind.to_ascii_lowercase();
    if let Ok(guard) = CACHE.read() {
        if let Some(hit) = guard.get(&key) {
            return Some(hit);
        }
    }
    let fields = load_resource_ref(&key)?.get("fields")?;
    let computed: &'static [LeafFieldPath] =
        Box::leak(get_leaf_field_paths(fields).into_boxed_slice());
    let mut guard = CACHE.write().ok()?;
    Some(*guard.entry(key).or_insert(computed))
}

/// Metadata keys that are never surfaced as editable form fields.
/// Exact matches only -- `"enum:"` is a literal field name, not a prefix.
#[inline]
fn is_skipped(name: &str) -> bool {
    matches!(
        name,
        "status"
            | "managedFields"
            | "creationTimestamp"
            | "deletionTimestamp"
            | "deletionGracePeriodSeconds"
            | "selfLink"
            | "resourceVersion"
            | "uid"
            | "generation"
            | "ownerReferences"
            | "finalizers"
            | "generateName"
            | "enum:"
    )
}

fn _walk_leaf_paths(
    fields: &serde_json::Map<String, Value>,
    prefix: &str,
    results: &mut Vec<LeafFieldPath>,
) {
    results.reserve(fields.len());
    for (name, info) in fields {
        if is_skipped(name) {
            continue;
        }
        if prefix.is_empty() && (name == "apiVersion" || name == "kind") {
            continue;
        }
        let ftype = info
            .get("type")
            .and_then(|v| v.as_str())
            .unwrap_or("string");
        let required = info
            .get("required")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let mut path = String::with_capacity(prefix.len() + name.len() + 4);
        if !prefix.is_empty() {
            path.push_str(prefix);
            path.push('.');
        }
        if ftype.starts_with("[]") {
            path.push_str(name);
            path.push_str("[]");
        } else {
            path.push_str(name);
        }

        if let Some(Value::Object(child_fields)) = info.get("fields") {
            if !child_fields.is_empty() {
                _walk_leaf_paths(child_fields, &path, results);
                continue;
            }
        }
        results.push(LeafFieldPath {
            path,
            field_type: ftype.to_string(),
            required,
        });
    }
}

pub fn resolve_field_path_ref<'a>(fields: &'a Value, dotted_path: &str) -> Option<&'a Value> {
    let parts: Vec<&str> = dotted_path.split('.').collect();
    if parts.is_empty() {
        return None;
    }
    let mut current = fields;
    for (i, part) in parts.iter().enumerate() {
        if i == parts.len() - 1 {
            return current.get(*part);
        }
        current = current.get(*part)?.get("fields")?;
    }
    None
}

pub fn resolve_field_path(fields: &Value, dotted_path: &str) -> Option<Value> {
    resolve_field_path_ref(fields, dotted_path).cloned()
}
