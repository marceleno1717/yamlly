use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use serde_json::Value;

/// Kind of non-blocking `kubectl` manifest operation piped via `stdin` (`-f -`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KubectlOpKind {
    /// `kubectl apply --dry-run=server -f -`
    DryRunServer,
    /// `kubectl diff -f -`
    Diff,
    /// `kubectl apply -f -`
    Apply,
}

impl KubectlOpKind {
    pub const fn label(self) -> &'static str {
        match self {
            Self::DryRunServer => "Server Dry-Run",
            Self::Diff => "Cluster Diff",
            Self::Apply => "Cluster Apply",
        }
    }
}

/// Completed result of a background [`KubectlOpKind`] invocation.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KubectlOpResult {
    pub op: KubectlOpKind,
    pub ok: bool,
    /// `true` when `kubectl diff` exits with code `0` (manifest is identical to live object).
    pub no_changes: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Build the CLI argument list for a stdin-piped `kubectl` manifest operation.
pub fn build_kubectl_stdin_args(
    op: KubectlOpKind,
    context: Option<&str>,
    namespace: Option<&str>,
) -> Vec<String> {
    let mut args = Vec::with_capacity(8);
    if let Some(ctx) = context.map(str::trim).filter(|s| !s.is_empty()) {
        args.push(format!("--context={ctx}"));
    }
    if let Some(ns) = namespace.map(str::trim).filter(|s| !s.is_empty()) {
        args.push("-n".to_string());
        args.push(ns.to_string());
    }
    match op {
        KubectlOpKind::DryRunServer => {
            args.push("apply".to_string());
            args.push("--dry-run=server".to_string());
        }
        KubectlOpKind::Diff => {
            args.push("diff".to_string());
        }
        KubectlOpKind::Apply => {
            args.push("apply".to_string());
        }
    }
    args.push("--request-timeout=5s".to_string());
    args.push("-f".to_string());
    args.push("-".to_string());
    args
}

/// Interpret the exit code, stdout, and stderr of a `kubectl` stdin operation.
///
/// Specifically handles `kubectl diff` exit codes:
/// - `0`: no differences (`ok = true, no_changes = true`)
/// - `1`: differences found (`ok = true, no_changes = false`)
/// - `> 1` or `None`: error (`ok = false`)
pub fn interpret_kubectl_output(
    op: KubectlOpKind,
    exit_code: Option<i32>,
    stdout: String,
    stderr: String,
) -> KubectlOpResult {
    let stdout = stdout.trim_end().to_string();
    let stderr = stderr.trim().to_string();
    match (op, exit_code) {
        (KubectlOpKind::Diff, Some(0)) => KubectlOpResult {
            op,
            ok: true,
            no_changes: true,
            stdout,
            stderr,
        },
        (KubectlOpKind::Diff, Some(1)) => KubectlOpResult {
            op,
            ok: true,
            no_changes: false,
            stdout,
            stderr,
        },
        (_, Some(0)) => KubectlOpResult {
            op,
            ok: true,
            no_changes: false,
            stdout,
            stderr,
        },
        _ => KubectlOpResult {
            op,
            ok: false,
            no_changes: false,
            stdout,
            stderr: if stderr.is_empty() {
                format!(
                    "kubectl {} exited with status {:?}",
                    op.label(),
                    exit_code.unwrap_or(-1)
                )
            } else {
                stderr
            },
        },
    }
}

/// Run a `kubectl` manifest operation synchronously by piping `yaml` over `stdin` (`-f -`).
pub fn run_kubectl_stdin(
    op: KubectlOpKind,
    yaml: &str,
    context: Option<&str>,
    namespace: Option<&str>,
) -> KubectlOpResult {
    let args = build_kubectl_stdin_args(op, context, namespace);
    let mut child = match Command::new("kubectl")
        .args(&args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(c) => c,
        Err(e) => {
            return KubectlOpResult {
                op,
                ok: false,
                no_changes: false,
                stdout: String::new(),
                stderr: format!("Failed to spawn kubectl: {e}"),
            };
        }
    };

    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(yaml.as_bytes());
    }

    match child.wait_with_output() {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            interpret_kubectl_output(op, output.status.code(), stdout, stderr)
        }
        Err(e) => KubectlOpResult {
            op,
            ok: false,
            no_changes: false,
            stdout: String::new(),
            stderr: format!("Failed waiting for kubectl: {e}"),
        },
    }
}

/// Spawn a background worker thread to run a `kubectl` stdin operation without blocking the TUI.
pub fn run_kubectl_stdin_async(
    op: KubectlOpKind,
    yaml: String,
    context: Option<String>,
    namespace: Option<String>,
    tx: Sender<KubectlOpResult>,
) {
    thread::spawn(move || {
        let res = run_kubectl_stdin(op, &yaml, context.as_deref(), namespace.as_deref());
        let _ = tx.send(res);
    });
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct ClusterStatus {
    pub is_checking: bool,
    pub connected: bool,
    pub context_name: Option<String>,
    pub namespace: String,
    pub error: Option<String>,
}

impl Default for ClusterStatus {
    fn default() -> Self {
        Self {
            is_checking: true,
            connected: false,
            context_name: None,
            namespace: "default".to_string(),
            error: None,
        }
    }
}

/// Result of an async `kubectl get <kind> -o json` fetch for the Cluster Import modal.
#[derive(Debug, Clone)]
pub struct ClusterFetchResult {
    pub kind: String,
    pub items: Vec<Value>,
    pub error: Option<String>,
}

pub fn check_connection() -> ClusterStatus {
    // 1. Get current context
    let ctx_output = Command::new("kubectl")
        .args(["config", "current-context"])
        .output();

    let context_name = match ctx_output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    };

    if context_name.is_none() {
        return ClusterStatus {
            is_checking: false,
            connected: false,
            context_name: None,
            namespace: "default".to_string(),
            error: Some("No current context".to_string()),
        };
    }

    // 2. Get current namespace
    let ns_output = Command::new("kubectl")
        .args(["config", "view", "--minify", "-o", "jsonpath={..namespace}"])
        .output();

    let namespace = match ns_output {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() {
                "default".to_string()
            } else {
                s
            }
        }
        _ => "default".to_string(),
    };

    // 3. Test cluster reachability
    let test_output = Command::new("kubectl")
        .args(["get", "namespace", "default", "--request-timeout=2s"])
        .output();

    match test_output {
        Ok(out) if out.status.success() => ClusterStatus {
            is_checking: false,
            connected: true,
            context_name,
            namespace,
            error: None,
        },
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            ClusterStatus {
                is_checking: false,
                connected: false,
                context_name,
                namespace,
                error: Some(err),
            }
        }
        Err(e) => ClusterStatus {
            is_checking: false,
            connected: false,
            context_name,
            namespace,
            error: Some(e.to_string()),
        },
    }
}

/// Spawns a background worker to check connection without stalling the UI.
pub fn check_connection_async(tx: Sender<ClusterStatus>) {
    thread::spawn(move || {
        let status = check_connection();
        let _ = tx.send(status);
    });
}

/// Fetch live resources of `kind` from the cluster as JSON objects (`kubectl get <kind> -o json`).
pub fn get_cluster_resources(kind: &str, namespace: Option<&str>) -> ClusterFetchResult {
    let mut args = vec![
        "get".to_string(),
        kind.to_ascii_lowercase(),
        "-o".to_string(),
        "json".to_string(),
        "--request-timeout=5s".to_string(),
    ];
    if let Some(ns) = namespace.filter(|s| !s.trim().is_empty()) {
        args.push("-n".to_string());
        args.push(ns.to_string());
    } else {
        args.push("-A".to_string());
    }

    match Command::new("kubectl").args(&args).output() {
        Ok(out) if out.status.success() => {
            let stdout = String::from_utf8_lossy(&out.stdout);
            match serde_json::from_str::<Value>(&stdout) {
                Ok(doc) => {
                    let items = doc
                        .get("items")
                        .and_then(|v| v.as_array())
                        .cloned()
                        .unwrap_or_default();
                    ClusterFetchResult {
                        kind: kind.to_string(),
                        items,
                        error: None,
                    }
                }
                Err(e) => ClusterFetchResult {
                    kind: kind.to_string(),
                    items: Vec::new(),
                    error: Some(format!("Invalid JSON from kubectl: {e}")),
                },
            }
        }
        Ok(out) => {
            let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
            // If -A failed (e.g. cluster-scoped RBAC forbidden), retry without -A if namespace was None
            if namespace.is_none()
                && (stderr.contains("forbidden")
                    || stderr.contains("unknown flag")
                    || stderr.contains("not found"))
            {
                let fallback_args = vec![
                    "get".to_string(),
                    kind.to_ascii_lowercase(),
                    "-o".to_string(),
                    "json".to_string(),
                    "--request-timeout=5s".to_string(),
                ];
                if let Ok(fallback_out) = Command::new("kubectl").args(&fallback_args).output()
                    && fallback_out.status.success()
                {
                    let stdout = String::from_utf8_lossy(&fallback_out.stdout);
                    if let Ok(doc) = serde_json::from_str::<Value>(&stdout) {
                        let items = doc
                            .get("items")
                            .and_then(|v| v.as_array())
                            .cloned()
                            .unwrap_or_default();
                        return ClusterFetchResult {
                            kind: kind.to_string(),
                            items,
                            error: None,
                        };
                    }
                }
            }
            ClusterFetchResult {
                kind: kind.to_string(),
                items: Vec::new(),
                error: Some(if stderr.is_empty() {
                    "kubectl command failed".to_string()
                } else {
                    stderr
                }),
            }
        }
        Err(e) => ClusterFetchResult {
            kind: kind.to_string(),
            items: Vec::new(),
            error: Some(format!("Failed to run kubectl: {e}")),
        },
    }
}

/// Spawns a background worker to fetch cluster resources of `kind` without blocking the UI.
pub fn fetch_cluster_resources_async(
    kind: String,
    namespace: Option<String>,
    tx: Sender<ClusterFetchResult>,
) {
    thread::spawn(move || {
        let res = get_cluster_resources(&kind, namespace.as_deref());
        let _ = tx.send(res);
    });
}

/// Clean server-side runtime metadata from a cluster manifest before importing it into an editor tab.
pub fn clean_cluster_manifest(val: &Value) -> Value {
    let mut cloned = val.clone();
    if let Value::Object(map) = &mut cloned {
        map.remove("status");
        if let Some(Value::Object(meta)) = map.get_mut("metadata") {
            meta.remove("managedFields");
            meta.remove("uid");
            meta.remove("resourceVersion");
            meta.remove("generation");
            meta.remove("creationTimestamp");
            meta.remove("selfLink");
            if let Some(Value::Object(annotations)) = meta.get_mut("annotations") {
                annotations.remove("kubectl.kubernetes.io/last-applied-configuration");
                if annotations.is_empty() {
                    meta.remove("annotations");
                }
            }
        }
    }
    cloned
}

/// Extract key/value map entries from a cluster resource item based on the target field path,
/// matching `ktui`'s `ClusterImportScreen` behavior:
/// - If `target_path` contains `"selector"`, inspect:
///   1. `spec.selector.matchLabels` (Deployment/StatefulSet/DaemonSet)
///   2. `spec.selector` (Service)
///   3. `spec.template.metadata.labels` (Pod template)
///   4. `metadata.labels` (Pod/Resource labels)
/// - If `target_path` contains `"annotations"`, extract `metadata.annotations`.
/// - Otherwise extract `metadata.labels` (or fallback to pod template labels).
pub fn extract_map_from_cluster_item(item: &Value, target_path: &str) -> Vec<(String, String)> {
    let lower_path = target_path.to_ascii_lowercase();
    let map_obj = if lower_path.contains("selector") {
        item.pointer("/spec/selector/matchLabels")
            .and_then(|v| v.as_object())
            .or_else(|| item.pointer("/spec/selector").and_then(|v| v.as_object()))
            .or_else(|| {
                item.pointer("/spec/template/metadata/labels")
                    .and_then(|v| v.as_object())
            })
            .or_else(|| item.pointer("/metadata/labels").and_then(|v| v.as_object()))
    } else if lower_path.contains("annotation") {
        item.pointer("/metadata/annotations")
            .and_then(|v| v.as_object())
            .or_else(|| item.pointer("/metadata/labels").and_then(|v| v.as_object()))
    } else {
        item.pointer("/metadata/labels")
            .and_then(|v| v.as_object())
            .or_else(|| {
                item.pointer("/spec/template/metadata/labels")
                    .and_then(|v| v.as_object())
            })
    };

    let mut out = Vec::new();
    if let Some(obj) = map_obj {
        for (k, v) in obj {
            let val_str = match v {
                Value::String(s) => s.clone(),
                Value::Null => String::new(),
                other => other.to_string(),
            };
            out.push((k.clone(), val_str));
        }
    }
    out.sort_by(|a, b| a.0.cmp(&b.0));
    out
}

/// Retrieve all configured Kubernetes context names from `kubectl config get-contexts -o name`.
pub fn get_all_contexts() -> Vec<String> {
    match Command::new("kubectl")
        .args(["config", "get-contexts", "-o", "name"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut list: Vec<String> = s
                .lines()
                .map(|l| l.trim().to_string())
                .filter(|l| !l.is_empty())
                .collect();
            list.sort();
            list
        }
        _ => Vec::new(),
    }
}

/// Retrieve the active Kubernetes context name via `kubectl config current-context`.
pub fn get_current_context() -> Option<String> {
    match Command::new("kubectl")
        .args(["config", "current-context"])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if s.is_empty() { None } else { Some(s) }
        }
        _ => None,
    }
}

/// Switch the active Kubernetes context via `kubectl config use-context <name>`.
pub fn use_context(context_name: &str) -> Result<(), String> {
    match Command::new("kubectl")
        .args(["config", "use-context", context_name])
        .output()
    {
        Ok(out) if out.status.success() => Ok(()),
        Ok(out) => {
            let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
            Err(if err.is_empty() {
                format!("Failed to switch to context {context_name}")
            } else {
                err
            })
        }
        Err(e) => Err(format!("Failed to execute kubectl: {e}")),
    }
}

/// Fetch all namespaces from the current cluster.
pub fn get_all_namespaces() -> Vec<String> {
    match Command::new("kubectl")
        .args([
            "get",
            "namespaces",
            "-o",
            "jsonpath={.items[*].metadata.name}",
            "--request-timeout=3s",
        ])
        .output()
    {
        Ok(out) if out.status.success() => {
            let s = String::from_utf8_lossy(&out.stdout);
            let mut list: Vec<String> = s
                .split_whitespace()
                .map(|ns| ns.trim().to_string())
                .filter(|ns| !ns.is_empty())
                .collect();
            if list.is_empty() {
                list = vec![
                    "default".to_string(),
                    "kube-system".to_string(),
                    "kube-public".to_string(),
                ];
            }
            list.sort();
            list
        }
        _ => vec![
            "default".to_string(),
            "kube-system".to_string(),
            "kube-public".to_string(),
        ],
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_clean_cluster_manifest_strips_runtime_metadata() {
        let raw = json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {
                "name": "nginx-demo",
                "namespace": "production",
                "uid": "bf86d5e9-89b5-4b53-8472-a72049d5bfcb",
                "resourceVersion": "123456",
                "generation": 2,
                "creationTimestamp": "2026-04-12T10:15:30Z",
                "selfLink": "/apis/apps/v1/namespaces/production/deployments/nginx-demo",
                "managedFields": [
                    { "manager": "kubectl-client-side-apply", "operation": "Update" }
                ],
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{\"kind\":\"Deployment\"}",
                    "team": "frontend"
                },
                "labels": {
                    "app": "nginx"
                }
            },
            "spec": {
                "replicas": 3
            },
            "status": {
                "availableReplicas": 3,
                "conditions": [
                    { "type": "Available", "status": "True" }
                ]
            }
        });

        let cleaned = clean_cluster_manifest(&raw);

        assert!(cleaned.get("status").is_none(), "status must be stripped");
        let meta = cleaned.get("metadata").expect("metadata present");
        assert!(meta.get("uid").is_none(), "uid stripped");
        assert!(
            meta.get("resourceVersion").is_none(),
            "resourceVersion stripped"
        );
        assert!(meta.get("generation").is_none(), "generation stripped");
        assert!(
            meta.get("creationTimestamp").is_none(),
            "creationTimestamp stripped"
        );
        assert!(meta.get("selfLink").is_none(), "selfLink stripped");
        assert!(
            meta.get("managedFields").is_none(),
            "managedFields stripped"
        );

        let annotations = meta.get("annotations").expect("user annotations preserved");
        assert!(
            annotations
                .get("kubectl.kubernetes.io/last-applied-configuration")
                .is_none(),
            "last-applied-configuration must be stripped"
        );
        assert_eq!(
            annotations.get("team").and_then(|v| v.as_str()),
            Some("frontend"),
            "user annotation team must be preserved"
        );

        assert_eq!(
            meta.pointer("/labels/app").and_then(|v| v.as_str()),
            Some("nginx")
        );
        assert_eq!(
            cleaned.pointer("/spec/replicas").and_then(|v| v.as_i64()),
            Some(3)
        );
    }

    #[test]
    fn test_clean_cluster_manifest_removes_empty_annotations() {
        let raw = json!({
            "apiVersion": "v1",
            "kind": "ConfigMap",
            "metadata": {
                "name": "app-config",
                "annotations": {
                    "kubectl.kubernetes.io/last-applied-configuration": "{\"kind\":\"ConfigMap\"}"
                }
            }
        });

        let cleaned = clean_cluster_manifest(&raw);
        assert!(
            cleaned.pointer("/metadata/annotations").is_none(),
            "annotations object should be removed if only last-applied-configuration was present"
        );
    }

    #[test]
    fn test_extract_map_from_cluster_item_handles_selectors_services_and_pods() {
        // Deployment with spec.selector.matchLabels
        let deployment = json!({
            "kind": "Deployment",
            "metadata": {
                "labels": { "app": "web", "version": "1.0" }
            },
            "spec": {
                "selector": {
                    "matchLabels": { "app": "web", "tier": "frontend" }
                },
                "template": {
                    "metadata": {
                        "labels": { "app": "web", "tier": "frontend", "track": "stable" }
                    }
                }
            }
        });

        let dep_selector = extract_map_from_cluster_item(&deployment, "spec.selector.matchLabels");
        assert_eq!(
            dep_selector,
            vec![
                ("app".to_string(), "web".to_string()),
                ("tier".to_string(), "frontend".to_string())
            ]
        );

        // Service with spec.selector
        let service = json!({
            "kind": "Service",
            "metadata": { "name": "web-svc" },
            "spec": {
                "selector": { "app": "web", "role": "ui" }
            }
        });

        let svc_selector = extract_map_from_cluster_item(&service, "spec.selector");
        assert_eq!(
            svc_selector,
            vec![
                ("app".to_string(), "web".to_string()),
                ("role".to_string(), "ui".to_string())
            ]
        );

        // Pod fallback to metadata.labels
        let pod = json!({
            "kind": "Pod",
            "metadata": {
                "labels": { "app": "db", "role": "primary" }
            }
        });

        let pod_selector = extract_map_from_cluster_item(&pod, "spec.selector.matchLabels");
        assert_eq!(
            pod_selector,
            vec![
                ("app".to_string(), "db".to_string()),
                ("role".to_string(), "primary".to_string())
            ]
        );
    }

    #[test]
    fn test_build_kubectl_stdin_args_for_dry_run_diff_and_apply() {
        let dry_run_args = build_kubectl_stdin_args(
            KubectlOpKind::DryRunServer,
            Some("prod-cluster"),
            Some("payments"),
        );
        assert_eq!(
            dry_run_args,
            vec![
                "--context=prod-cluster",
                "-n",
                "payments",
                "apply",
                "--dry-run=server",
                "--request-timeout=5s",
                "-f",
                "-",
            ]
        );

        let diff_args = build_kubectl_stdin_args(KubectlOpKind::Diff, None, Some("default"));
        assert_eq!(
            diff_args,
            vec!["-n", "default", "diff", "--request-timeout=5s", "-f", "-"]
        );

        let apply_args = build_kubectl_stdin_args(KubectlOpKind::Apply, Some("minikube"), None);
        assert_eq!(
            apply_args,
            vec![
                "--context=minikube",
                "apply",
                "--request-timeout=5s",
                "-f",
                "-"
            ]
        );
    }

    #[test]
    fn test_interpret_kubectl_output_handles_diff_exit_codes_and_errors() {
        // Diff exit code 0 -> ok + no_changes
        let diff_zero =
            interpret_kubectl_output(KubectlOpKind::Diff, Some(0), String::new(), String::new());
        assert!(diff_zero.ok);
        assert!(diff_zero.no_changes);

        // Diff exit code 1 -> ok + changes present
        let diff_one = interpret_kubectl_output(
            KubectlOpKind::Diff,
            Some(1),
            "--- LIVE\n+++ MERGED\n+  replicas: 3\n".to_string(),
            String::new(),
        );
        assert!(diff_one.ok);
        assert!(!diff_one.no_changes);
        assert!(diff_one.stdout.contains("+  replicas: 3"));

        // Diff exit code 2 -> error
        let diff_err = interpret_kubectl_output(
            KubectlOpKind::Diff,
            Some(2),
            String::new(),
            "Error from server (Forbidden): deployments.apps is forbidden".to_string(),
        );
        assert!(!diff_err.ok);
        assert!(!diff_err.no_changes);
        assert!(diff_err.stderr.contains("Forbidden"));

        // DryRunServer exit code 1 -> error (unlike Diff, exit 1 for apply is a failure)
        let dry_err = interpret_kubectl_output(
            KubectlOpKind::DryRunServer,
            Some(1),
            String::new(),
            "admission webhook denied the request".to_string(),
        );
        assert!(!dry_err.ok);
        assert!(dry_err.stderr.contains("admission webhook"));
    }
}
