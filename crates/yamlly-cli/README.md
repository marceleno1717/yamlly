<div align="center">
  <h1>🚢 yamlly</h1>
  <p><strong>An interactive terminal UI (TUI) for building and editing Kubernetes YAML manifests.</strong></p>

  <p>
    <a href="https://github.com/marceleno1717/yamlly/actions/workflows/ci.yml"><img src="https://github.com/marceleno1717/yamlly/actions/workflows/ci.yml/badge.svg" alt="CI Status"></a>
    <a href="https://crates.io/crates/yamlly-cli"><img src="https://img.shields.io/crates/v/yamlly-cli" alt="Crates.io version"></a>
    <a href="https://pypi.org/project/yamlly-cli/"><img src="https://img.shields.io/pypi/v/yamlly-cli" alt="PyPI version"></a>
    <a href="https://www.npmjs.com/package/yamlly-cli"><img src="https://img.shields.io/npm/v/yamlly-cli" alt="npm version"></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/License-PolyForm%20Noncommercial-blue.svg" alt="License"></a>
  </p>
</div>

---

## ⚡ Why yamlly?

`yamlly` gives you a blazing-fast, schema-driven Kubernetes manifest studio directly inside your terminal—written in Rust and powered by [Ratatui](https://ratatui.rs/) and [ratcn](https://github.com/kristoferlund/ratcn).

Pick a resource, fill in the fields you need, search and mount optional properties from embedded Kubernetes schemas, import live resources from your cluster, and preview or save clean YAML anywhere on disk.

## ✨ Features

- 🏗️ **22 Embedded Kubernetes Schemas**: Zero external runtime files required (`Deployment`, `Pod`, `Service`, `Ingress`, `ConfigMap`, `Secret`, `StatefulSet`, `DaemonSet`, `Job`, `CronJob`, `PersistentVolume`, `PersistentVolumeClaim`, `HorizontalPodAutoscaler`, `NetworkPolicy`, `Role`, `ClusterRole`, `RoleBinding`, `ClusterRoleBinding`, `ServiceAccount`, `Namespace`, `ResourceQuota`, `LimitRange`).
- 🔍 **Interactive Field Picker (`+ Fields`)**: Search and mount individual optional fields or entire nested categories into your form, and remove added fields/categories anytime (`[ ✕ ]` or `Ctrl+D`).
- ☸️ **Live Cluster Integration (`Ctrl+I` / `Ctrl+K`)**: Switch Kubernetes contexts, inspect live namespaces, and import full manifests or label/selector maps directly from a running cluster.
- 🗂️ **Multi-Tab Workspace**: Edit multiple resources simultaneously with per-tab dirty tracking (`●`) and unsaved-change protection.
- 👁️ **Live YAML Preview & Filesystem Browser (`Ctrl+S` / `p`)**: Preview syntax-highlighted YAML and save to any relative or absolute path with an interactive directory picker.
- 🎨 **Multiple Built-in Themes (`t`)**: Cycle through curated terminal color palettes on the fly.

## 🚀 Installation

Installing `yamlly-cli` from any registry provides both the **`yamlly`** and **`ymly`** commands in your terminal.

### Option A: Cargo (Rust)

```bash
cargo install yamlly-cli
```

### Option B: PyPI (`pip` / `pipx` / `uv`)

```bash
pipx install yamlly-cli
# or
pip install yamlly-cli
# or run directly with uvx
uvx --from yamlly-cli yamlly
```

### Option C: npm / npx (Node.js)

```bash
# Install globally
npm install -g yamlly-cli

# Or run directly without installing
npx --package=yamlly-cli yamlly
```

## ⌨️ Usage & Keybindings

You can run the CLI using either `yamlly` or the shorthand `ymly`:

```bash
yamlly                  # Launch interactive TUI (or `ymly`)
yamlly deployment.yaml  # Open an existing YAML manifest (or `ymly deployment.yaml`)
yamlly --list           # List all supported Kubernetes resource kinds
yamlly --emit Deployment # Emit a starter YAML manifest to stdout
```

| Key | Action |
| :--- | :--- |
| `Tab` / `Shift+Tab` | Navigate between interactive controls & fields |
| `Enter` / `Space` | Activate button, toggle checkbox, or open dropdown/modal |
| `Ctrl+D` | Remove focused added optional field |
| `Ctrl+I` | Import manifest or labels from active Kubernetes cluster |
| `Ctrl+K` / `c` | Open Kubernetes Context Switcher |
| `Ctrl+V` | Validate active resource form |
| `Ctrl+S` / `p` | Open YAML Preview & Save dialog |
| `Ctrl+W` | Close active tab |
| `t` | Cycle color theme |
| `q` | Quit application |

## 📄 License

Licensed under the [PolyForm Noncommercial License 1.0.0](LICENSE).
