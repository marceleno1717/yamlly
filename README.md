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

  <p>
    <img src="https://raw.githubusercontent.com/marceleno1717/yamlly/main/assets/demo.svg" alt="yamlly interactive TUI demo" width="100%">
  </p>
</div>

---

## ⚡ Why yamlly?

`yamlly` gives you a blazing-fast, schema-driven Kubernetes manifest studio directly inside your terminal—written in Rust and powered by [Ratatui](https://ratatui.rs/) and [ratcn](https://github.com/kristoferlund/ratcn).

Pick a resource, fill in the fields you need, search and mount optional properties from embedded Kubernetes schemas, import live resources from your cluster, and preview or save clean YAML anywhere on disk.

## ✨ Features

- ⚡ **Express & Advanced Editing Modes (`m`)**:
  - **Express Mode** (default): Streamlined forms displaying only essential fields (3–7 per resource). Fill in a name and image and get a production-ready manifest in seconds.
  - **Smart Metadata Auto-Linking**: Typing `metadata.name` automatically propagates to `metadata.labels.app`, selectors, and primary container names. Manually editing any field cleanly decouples it without breaking the rest.
  - **Workspace Tab Switch**: Switch views effortlessly via the workspace window tabs (`[ ● Express ]` / `[ ◆ Advanced ]`) or press `m` anytime.
- ⚡ **Quick Add Presets (`+ Quick` / `Ctrl+A`)**: Mount common production blocks in one keystroke—including CPU/Memory Resources, Liveness/Readiness Probes, Tolerations, Node Selectors, Affinity, and Volumes.
- 🌐 **Optimized for SSH & Remote Sessions (`⚡ SSH`)**:
  - Ultra-responsive over slow or jittery SSH connections.
  - Single-flush 64 KB buffered I/O with synchronized terminal updates (DEC 2026).
  - Mouse-move filtering suppresses redraw floods; transparent background (`Color::Reset`) preserves terminal themes.
  - Auto-detected or configurable via `~/.config/yamlly/config.toml`.
- ⚙️ **Persistent User Configuration**: Auto-manages user preferences in `~/.config/yamlly/config.toml` (persists editing mode, color theme, default namespace, and SSH settings).
- 🏗️ **22 Embedded Kubernetes Schemas**: Zero external runtime files required (`Deployment`, `Pod`, `Service`, `Ingress`, `ConfigMap`, `Secret`, `StatefulSet`, `DaemonSet`, `Job`, `CronJob`, `PersistentVolume`, `PersistentVolumeClaim`, `HorizontalPodAutoscaler`, `NetworkPolicy`, `Role`, `ClusterRole`, `RoleBinding`, `ClusterRoleBinding`, `ServiceAccount`, `Namespace`, `ResourceQuota`, `LimitRange`).
- 🔍 **Interactive Field Picker (`+ Fields`)**: Search and mount individual optional fields or entire nested categories into your form, and remove added fields/categories anytime (`[ ✕ ]` or `Ctrl+D`).
- ☸️ **Live Cluster Integration & Workflows**:
  - **Server-Side Dry-Run (`d`)**: Validate manifests against admission webhooks and API schemas via `kubectl apply --dry-run=server` before saving.
  - **Live Cluster Diff (`f`)**: Inspect colorized unified diffs (`+` / `-` / `@@`) between your edited manifest and deployed cluster resources.
  - **Guarded Cluster Apply (`a`)**: Deploy manifests directly to your active cluster with an interactive safety confirmation guard.
  - **Cluster Import with Namespace Dropdown**: Switch contexts (`Ctrl+K` / `c`), refresh namespaces, and import live resources from any namespace with session persistence.
- 🔐 **Secret Data Base64 / Plain Text Toggle**: Switch effortlessly between Base64 and decoded plain text when editing Secret `data`, with automatic Base64 encoding when applied.
- 🗂️ **Multi-Tab Workspace**: Edit multiple resources simultaneously with per-tab dirty tracking (`●`) and unsaved-change protection.
- 👁️ **Live YAML Preview & Filesystem Browser (`Ctrl+S` / `p`)**: Preview syntax-highlighted YAML and save to any relative or absolute path with an interactive directory picker.
- 🎨 **Multiple Built-in Themes (`t`)**: Cycle through curated terminal color palettes on the fly.
- 🛡️ **Clean Diagnostics**: Sanitizes noisy client-go / klog stack traces into actionable, friendly error messages.

## 🚀 Installation

Installing `yamlly-cli` from any registry provides both the **`yamlly`** and **`ymly`** commands in your terminal.

### Option A: Cargo (Rust)

```bash
# Fast binary install via cargo-binstall (~1s)
cargo binstall yamlly-cli

# Or compile from source
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
| `m` | Toggle between **Express** and **Advanced** schema mode |
| `Ctrl+A` | Open **Quick Add** presets modal (`+ Quick`) |
| `Ctrl+D` | Remove focused added optional field |
| `Ctrl+I` | Import manifest or labels from active Kubernetes cluster |
| `Ctrl+K` / `c` | Open Kubernetes Context Switcher |
| `Ctrl+V` | Validate active resource form |
| `Ctrl+S` / `p` | Open YAML Preview & Save dialog |
| `d` *(in Preview)* | Run server-side dry-run validation (`kubectl apply --dry-run=server`) |
| `f` *(in Preview)* | Toggle live cluster diff view (`kubectl diff`) |
| `y` *(in Preview)* | Switch back to YAML manifest view |
| `a` *(in Preview)* | Apply manifest to active cluster (`kubectl apply`) |
| `Ctrl+E` / `Ctrl+B` *(in Secret Map)* | Toggle Base64 ↔ Plain text editing mode |
| `Ctrl+W` | Close active tab |
| `t` | Cycle color theme |
| `q` | Quit application |

## 📄 License

Licensed under the [PolyForm Noncommercial License 1.0.0](LICENSE).
