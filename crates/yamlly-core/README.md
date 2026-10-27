# yamlly-core

Core schema loader, form tree builder, path state engine, and YAML emitter for [`yamlly`](https://crates.io/crates/yamlly).

- **Embedded Schemas**: Compile-time embedded Kubernetes resource definitions for 22 standard resource kinds.
- **Form Tree Builder**: Hierarchical path grouping, field classification, and array item materialization.
- **State & Dirty Engine**: Nested JSON path access (`get_nested`, `set_nested`) and zero-allocation dirty-state comparison (`is_dirty`).
- **YAML Emitter**: Clean 2-space mapping and sequence YAML serializer with automatic null/empty pruning.
