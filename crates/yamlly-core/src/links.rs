use crate::state::set_nested;
use serde_json::Value;
use std::collections::HashSet;

/// Definition of an auto-propagated link from a source field path to a target field path.
pub struct LinkRule {
    pub source: &'static str,
    pub target: &'static str,
    pub transform: fn(&str) -> Value,
}

impl LinkRule {
    pub const fn new(
        source: &'static str,
        target: &'static str,
        transform: fn(&str) -> Value,
    ) -> Self {
        Self {
            source,
            target,
            transform,
        }
    }
}

fn as_string(val: &str) -> Value {
    Value::String(val.to_string())
}

/// Retrieve the active auto-link rules for a given Kubernetes resource kind.
pub fn get_link_rules_for_kind(kind: &str) -> Vec<LinkRule> {
    match kind.to_ascii_lowercase().as_str() {
        "deployment" | "statefulset" | "daemonset" => vec![
            LinkRule::new("metadata.name", "metadata.labels.app", as_string),
            LinkRule::new("metadata.name", "spec.selector.matchLabels.app", as_string),
            LinkRule::new(
                "metadata.name",
                "spec.template.metadata.labels.app",
                as_string,
            ),
            LinkRule::new(
                "metadata.name",
                "spec.template.spec.containers[0].name",
                as_string,
            ),
        ],
        "pod" => vec![
            LinkRule::new("metadata.name", "metadata.labels.app", as_string),
            LinkRule::new("metadata.name", "spec.containers[0].name", as_string),
        ],
        "service" => vec![
            LinkRule::new("metadata.name", "metadata.labels.app", as_string),
            LinkRule::new("metadata.name", "spec.selector.app", as_string),
        ],
        "job" => vec![
            LinkRule::new("metadata.name", "metadata.labels.app", as_string),
            LinkRule::new(
                "metadata.name",
                "spec.template.spec.containers[0].name",
                as_string,
            ),
            LinkRule::new("metadata.name", "spec.template.spec.restartPolicy", |_| {
                Value::String("Never".to_string())
            }),
        ],
        "cronjob" => vec![
            LinkRule::new("metadata.name", "metadata.labels.app", as_string),
            LinkRule::new(
                "metadata.name",
                "spec.jobTemplate.spec.template.spec.containers[0].name",
                as_string,
            ),
            LinkRule::new(
                "metadata.name",
                "spec.jobTemplate.spec.template.spec.restartPolicy",
                |_| Value::String("OnFailure".to_string()),
            ),
        ],
        "configmap" | "secret" => vec![LinkRule::new(
            "metadata.name",
            "metadata.labels.app",
            as_string,
        )],
        _ => Vec::new(),
    }
}

/// Helper struct tracking which auto-propagated targets have been decoupled by manual edits.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LinkTracker {
    /// Target paths that have been explicitly edited by the user and should no longer auto-update.
    pub unlinked_targets: HashSet<String>,
}

impl LinkTracker {
    pub fn new() -> Self {
        Self {
            unlinked_targets: HashSet::new(),
        }
    }

    /// Mark a target path as manually edited / decoupled.
    pub fn unlink(&mut self, target_path: &str) {
        self.unlinked_targets.insert(target_path.to_string());
    }

    /// Check whether a target path is still actively linked.
    pub fn is_linked(&self, target_path: &str) -> bool {
        !self.unlinked_targets.contains(target_path)
    }

    /// Apply all active auto-link rules when a source field is updated.
    /// Returns the list of target paths that were updated.
    pub fn propagate_links(
        &self,
        kind: &str,
        changed_source: &str,
        new_source_val: &str,
        data: &mut Value,
    ) -> Vec<String> {
        let rules = get_link_rules_for_kind(kind);
        let mut updated = Vec::new();

        for rule in rules {
            if rule.source == changed_source && self.is_linked(rule.target) {
                let transformed = (rule.transform)(new_source_val);
                let segs: Vec<&str> = rule.target.split('.').collect();
                set_nested(data, &segs, transformed);
                updated.push(rule.target.to_string());
            }
        }
        updated
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::get_nested;
    use serde_json::json;

    #[test]
    fn test_deployment_auto_links_propagation() {
        let mut tracker = LinkTracker::new();
        let mut doc = json!({
            "apiVersion": "apps/v1",
            "kind": "Deployment",
            "metadata": {}
        });

        let updated =
            tracker.propagate_links("Deployment", "metadata.name", "payments-api", &mut doc);
        assert_eq!(updated.len(), 4);

        assert_eq!(
            get_nested(&doc, &["metadata", "labels", "app"]),
            Some(&Value::String("payments-api".to_string()))
        );
        assert_eq!(
            get_nested(&doc, &["spec", "selector", "matchLabels", "app"]),
            Some(&Value::String("payments-api".to_string()))
        );
        assert_eq!(
            get_nested(
                &doc,
                &["spec", "template", "spec", "containers", "0", "name"]
            ),
            Some(&Value::String("payments-api".to_string()))
        );

        // Now manually unlink containers[0].name
        tracker.unlink("spec.template.spec.containers[0].name");
        assert!(!tracker.is_linked("spec.template.spec.containers[0].name"));

        // Changing metadata.name again should update labels, but NOT container name
        let updated2 =
            tracker.propagate_links("Deployment", "metadata.name", "billing-svc", &mut doc);
        assert!(!updated2.contains(&"spec.template.spec.containers[0].name".to_string()));
        assert_eq!(
            get_nested(&doc, &["metadata", "labels", "app"]),
            Some(&Value::String("billing-svc".to_string()))
        );
        // Container name retains its previous decoupled value
        assert_eq!(
            get_nested(
                &doc,
                &["spec", "template", "spec", "containers", "0", "name"]
            ),
            Some(&Value::String("payments-api".to_string()))
        );
    }
}
