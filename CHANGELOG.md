# yamlly v1.1.1

`yamlly` v1.1.1 is a combined feature and performance release introducing **Express & Advanced Editing Modes**, **Quick Add Presets**, persistent user configuration via **`~/.config/yamlly/config.toml`**, high-performance **SSH / Low-Bandwidth Optimizations**, and **pinned tab strip navigation**.

---

## ⚡ What's New

### 1. Express & Advanced Schema Modes (`m`)
- **First-Time Default**: Every resource now opens in **Express Mode** by default, displaying only essential fields (typically 3–7 fields per resource) needed to produce a valid Kubernetes manifest in seconds.
- **Auto-Linking & Propagation**:
  - Typing `metadata.name` automatically sets `metadata.labels.app`, `spec.selector.matchLabels.app`, `spec.template.metadata.labels.app`, and `spec.template.spec.containers[0].name`.
  - Manual decoupling: Editing any linked field directly unlinks it from subsequent `metadata.name` updates without disrupting the rest of the form.
- **Mode Switch Buttons**: Switch seamlessly between views via bordered buttons with active highlighting (`[ Express ⚡ ]` and `[ Advanced ⚙ ]`), or press `m` anytime.

### 2. Quick Add Presets (`+ Quick` / `Ctrl+A`)
- Mount common, production-grade Kubernetes configuration blocks in one click:
  - **Resources**: Requests and limits for CPU and memory.
  - **Probes**: Configures HTTP `livenessProbe` and `readinessProbe`.
  - **Tolerations**: Common node tolerations.
  - **Node Selector**: Standard node selector matching.
  - **Node Affinity**: Required and preferred pod scheduling constraints.
  - **EmptyDir Volume**: Scratch volume and container volume mount.
- Full keyboard (Up/Down/Enter) and mouse selection support with visual radio markers (`● ` / `○ `).

### 3. Persistent User Configuration (`~/.config/yamlly/config.toml`)
- Fully XDG-compliant persistent configuration automatically initialized and saved:
  ```toml
  # ~/.config/yamlly/config.toml
  mode = "express"            # "express" or "advanced" (backwards-compatible with "basic")
  theme = "Dark"              # Saved color theme
  default_namespace = "default"
  ssh_mode = "auto"           # "auto", "always", or "never"
  ```
- Changes to editing mode (`m`) and color theme (`t`) persist automatically across sessions.

### 4. Remote & SSH Session Optimizations (`⚡ SSH`)
- **Zero-Lag Terminal Experience**:
  - Auto-detects SSH sessions from `$SSH_CONNECTION` and `$SSH_CLIENT` (or configure via `ssh_mode`).
  - **64 KB Buffered Output**: Consolidates terminal escape sequences into single network flushes to eliminate interactive latency over high-RTT or low-bandwidth links.
  - **Synchronized Updates**: Encloses terminal renders in DEC 2026 mode to prevent screen tearing and flicker.
  - **Mouse Move Suppression**: Filters out redundant `MouseKind::Moved` events to stop unnecessary redraw flushes.
  - **Transparent Background**: Renders background cells using `Color::Reset` to match native SSH terminal emulators.
  - **Visual Indicator**: Shows `⚡ SSH` pill in the top header bar when remote optimizations are active.

### 5. Pinned Tab Strip Overflow Navigation
- Fixed-grid navigation controls: left arrow `◀`, right arrow `▶`, and tab counter `(X/Y)` stay anchored in place without shifting when switching between tabs of varying title lengths.
- Keyboard cycling: `[` for previous tab, `]` for next tab.

---

## ⌨️ Summary of New Shortcuts

| Key | Action |
| :--- | :--- |
| `m` | Toggle between **Express** and **Advanced** schema mode |
| `Ctrl+A` | Open **Quick Add** presets modal (`+ Quick`) |
| `[` / `]` | Cycle active tab backward / forward |

---

## 🧪 Quality & Test Coverage
- **100% Passing Tests**: All 123 unit and integration tests passing (`cargo test --all`).
- **Clean Clippy & Formatting**: Passed `cargo clippy --all-targets -- -D warnings` and `cargo fmt --all -- --check`.
