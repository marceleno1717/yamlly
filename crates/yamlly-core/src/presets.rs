//! Curated Quick Add presets for instantly mounting commonly needed Kubernetes manifest blocks.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QuickAddPreset {
    pub id: &'static str,
    pub group: &'static str,
    pub title: &'static str,
    pub description: &'static str,
    pub paths: &'static [&'static str],
}

pub const PRESETS: &[QuickAddPreset] = &[
    QuickAddPreset {
        id: "tolerations",
        group: "Scheduling",
        title: "Tolerations",
        description: "Pod tolerations for node taints (operator, effect, key, value)",
        paths: &[
            "P.tolerations[].key",
            "P.tolerations[].operator",
            "P.tolerations[].value",
            "P.tolerations[].effect",
            "P.tolerations[].tolerationSeconds",
        ],
    },
    QuickAddPreset {
        id: "node_selector",
        group: "Scheduling",
        title: "Node Selector",
        description: "nodeSelector key-value label query map",
        paths: &["P.nodeSelector"],
    },
    QuickAddPreset {
        id: "node_affinity",
        group: "Scheduling",
        title: "Node Affinity (Required)",
        description: "Hard node affinity rules (requiredDuringScheduling)",
        paths: &[
            "P.affinity.nodeAffinity.requiredDuringSchedulingIgnoredDuringExecution.nodeSelectorTerms[].matchExpressions[].key",
            "P.affinity.nodeAffinity.requiredDuringSchedulingIgnoredDuringExecution.nodeSelectorTerms[].matchExpressions[].operator",
            "P.affinity.nodeAffinity.requiredDuringSchedulingIgnoredDuringExecution.nodeSelectorTerms[].matchExpressions[].values",
        ],
    },
    QuickAddPreset {
        id: "pod_anti_affinity",
        group: "Scheduling",
        title: "Pod Anti-Affinity (Spread)",
        description: "Spread pods across different nodes or zones",
        paths: &[
            "P.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[].weight",
            "P.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[].podAffinityTerm.topologyKey",
            "P.affinity.podAntiAffinity.preferredDuringSchedulingIgnoredDuringExecution[].podAffinityTerm.labelSelector.matchLabels",
        ],
    },
    QuickAddPreset {
        id: "resources",
        group: "Container",
        title: "Resources (Requests & Limits)",
        description: "CPU and memory request & limit guarantees",
        paths: &[
            "P.containers[].resources.requests.cpu",
            "P.containers[].resources.requests.memory",
            "P.containers[].resources.limits.cpu",
            "P.containers[].resources.limits.memory",
        ],
    },
    QuickAddPreset {
        id: "env_vars",
        group: "Container",
        title: "Environment Variables",
        description: "Name-value pairs passed directly to containers",
        paths: &[
            "P.containers[].env[].name",
            "P.containers[].env[].value",
        ],
    },
    QuickAddPreset {
        id: "probes",
        group: "Container",
        title: "Health Probes (Liveness & Readiness)",
        description: "HTTP GET liveness and readiness probe checks",
        paths: &[
            "P.containers[].livenessProbe.httpGet.path",
            "P.containers[].livenessProbe.httpGet.port",
            "P.containers[].readinessProbe.httpGet.path",
            "P.containers[].readinessProbe.httpGet.port",
        ],
    },
    QuickAddPreset {
        id: "security_context",
        group: "Security",
        title: "Security Context",
        description: "runAsNonRoot, runAsUser, and fsGroup controls",
        paths: &[
            "P.securityContext.runAsNonRoot",
            "P.securityContext.runAsUser",
            "P.securityContext.fsGroup",
        ],
    },
    QuickAddPreset {
        id: "service_account",
        group: "Security",
        title: "Service Account",
        description: "Bind custom ServiceAccount name to the pod",
        paths: &["P.serviceAccountName"],
    },
    QuickAddPreset {
        id: "annotations",
        group: "Metadata",
        title: "Annotations",
        description: "metadata.annotations key-value map",
        paths: &["metadata.annotations"],
    },
];

/// Resolve the `P.` pod-spec prefix for a given resource kind.
pub fn resolve_pod_prefix(kind: &str) -> Option<&'static str> {
    match kind.to_ascii_lowercase().as_str() {
        "pod" => Some("spec."),
        "deployment" | "statefulset" | "daemonset" | "job" => Some("spec.template.spec."),
        "cronjob" => Some("spec.jobTemplate.spec.template.spec."),
        _ => None,
    }
}

/// Retrieve and resolve all applicable presets for `kind`.
pub fn get_applicable_presets(kind: &str) -> Vec<(&'static QuickAddPreset, Vec<String>)> {
    let prefix = resolve_pod_prefix(kind);
    let mut result = Vec::new();

    for preset in PRESETS {
        let is_pod_scoped = preset.paths.iter().any(|p| p.starts_with("P."));
        if is_pod_scoped && prefix.is_none() {
            continue;
        }

        let resolved_paths: Vec<String> = preset
            .paths
            .iter()
            .map(|p| {
                if let Some(p_suffix) = p.strip_prefix("P.") {
                    let pref = prefix.unwrap_or("spec.");
                    format!("{pref}{p_suffix}")
                } else {
                    p.to_string()
                }
            })
            .collect();

        result.push((preset, resolved_paths));
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_presets_resolve_for_deployment() {
        let presets = get_applicable_presets("Deployment");
        assert!(presets.len() >= 8);
        let tolerations = presets.iter().find(|(p, _)| p.id == "tolerations").unwrap();
        assert!(tolerations.1[0].starts_with("spec.template.spec.tolerations"));
    }

    #[test]
    fn test_presets_resolve_for_pod() {
        let presets = get_applicable_presets("Pod");
        let tolerations = presets.iter().find(|(p, _)| p.id == "tolerations").unwrap();
        assert!(tolerations.1[0].starts_with("spec.tolerations"));
    }

    #[test]
    fn test_presets_for_service_only_metadata() {
        let presets = get_applicable_presets("Service");
        assert_eq!(presets.len(), 1);
        assert_eq!(presets[0].0.id, "annotations");
    }
}
