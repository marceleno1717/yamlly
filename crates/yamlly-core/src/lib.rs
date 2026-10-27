pub mod emitter;
pub mod field;
pub mod loader;
pub mod state;

pub use field::{
    build_form, build_path_tree, classify, group_sections, schema_lookup_path, FieldKind,
    FormField, PathTree, Row,
};

// Re-exports for convenience matching Python module structure
pub use emitter::{clean, clean_in_place, emit_yaml_from_value, emit_yaml_owned};
pub use loader::{
    get_all_resources, get_all_resources_ref, get_common_fields, get_common_fields_ref,
    get_leaf_field_paths, get_leaf_field_paths_cached, group_common_fields_by_section,
    load_resource, load_resource_ref, resolve_field_path, resolve_field_path_ref, LeafFieldPath,
    ResourceIndexEntry,
};
pub use state::{decode_path, encode_path, get_nested, is_dirty, normalize, set_nested, Segment};
