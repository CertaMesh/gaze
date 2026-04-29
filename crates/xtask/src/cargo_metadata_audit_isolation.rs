use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const AUDIT_PACKAGE: &str = "gaze-audit";
const SAFETY_NET_BASE_FEATURES: &[(&str, &str)] =
    &[("gaze", "safety-net"), ("gaze-recognizers", "safety-net")];
const SAFETY_NET_OPENAI_SUBPROCESS_FEATURES: &[(&str, &str)] = &[
    ("gaze", "safety-net"),
    ("gaze-recognizers", "safety-net"),
    ("gaze-recognizers", "safety-net-openai"),
];

// Phase 0 of todo #65 scopes these bans to safety-net feature graphs only.
// The default graph intentionally keeps the existing v0.5 NER dependency
// shape untouched so the v0.6 audit-shim drop is not blocked by v0.6.1 gates.
const SAFETY_NET_PROHIBITED_PACKAGES: &[&str] = &["reqwest", "hyper", "tokio", "ureq"];

// Existing NER deps are not part of the Phase 0 safety-net ban. Keep this
// exemption narrow and path-based: it allows the current `ort` downloader edge
// without permitting future safety-net modules or shared value crates to add
// their own network client dependency.
const SAFETY_NET_LEGACY_NER_EXEMPTIONS: &[LegacyNerExemption] = &[LegacyNerExemption {
    package: "ureq",
    required_path_member: "ort",
    reason: "legacy NER `ort` downloader edge; Phase 0 bans new safety-net network clients only",
}];

// Package-level exceptions require an explicit source comment. `gaze-cli` is
// audit-responsible because its audit command reads and purges audit metadata
// through the passive sink crate directly instead of using the `gaze` shim.
const AUDIT_RESPONSIBLE_PACKAGES: &[&str] = &["gaze-cli"];

pub fn run() -> Result<()> {
    let workspace = cargo_metadata(&["--no-deps"])?;
    let workspace_members = workspace_members_by_name(&workspace)?;

    for graph in GRAPH_CATEGORIES {
        let metadata = metadata_for_graph(graph, &workspace)?;
        check_graph(graph.label, &metadata, &workspace_members, graph.policy)?;
    }

    println!("cargo_metadata_audit_isolation: passed");
    Ok(())
}

const GRAPH_CATEGORIES: &[GraphCategory] = &[
    GraphCategory {
        label: "default",
        base_args: &[],
        planned_features: &[],
        policy: GraphPolicy {
            expect_gaze_audit_from_gaze: false,
            safety_net_dep_bans: false,
        },
    },
    GraphCategory {
        label: "no-default-features",
        base_args: &["--no-default-features"],
        planned_features: &[],
        policy: GraphPolicy {
            expect_gaze_audit_from_gaze: false,
            safety_net_dep_bans: false,
        },
    },
    GraphCategory {
        label: "safety-net-base",
        base_args: &["--no-default-features"],
        planned_features: SAFETY_NET_BASE_FEATURES,
        policy: GraphPolicy {
            expect_gaze_audit_from_gaze: false,
            safety_net_dep_bans: true,
        },
    },
    GraphCategory {
        label: "safety-net-openai-subprocess",
        base_args: &["--no-default-features"],
        planned_features: SAFETY_NET_OPENAI_SUBPROCESS_FEATURES,
        policy: GraphPolicy {
            expect_gaze_audit_from_gaze: false,
            safety_net_dep_bans: true,
        },
    },
];

#[derive(Debug, Clone, Copy)]
struct GraphCategory {
    label: &'static str,
    base_args: &'static [&'static str],
    planned_features: &'static [(&'static str, &'static str)],
    policy: GraphPolicy,
}

#[derive(Debug, Clone, Copy)]
struct GraphPolicy {
    expect_gaze_audit_from_gaze: bool,
    safety_net_dep_bans: bool,
}

#[derive(Debug, Clone, Copy)]
struct LegacyNerExemption {
    package: &'static str,
    required_path_member: &'static str,
    reason: &'static str,
}

fn metadata_for_graph(graph: &GraphCategory, workspace: &Metadata) -> Result<Metadata> {
    let mut args = graph
        .base_args
        .iter()
        .map(|arg| (*arg).to_string())
        .collect::<Vec<_>>();
    let enabled_features = available_planned_features(workspace, graph.planned_features)?;
    if !enabled_features.is_empty() {
        args.push("--features".to_string());
        args.push(enabled_features.join(","));
    }
    let arg_refs = args.iter().map(String::as_str).collect::<Vec<_>>();
    cargo_metadata(&arg_refs)
}

fn available_planned_features(
    workspace: &Metadata,
    planned_features: &[(&str, &str)],
) -> Result<Vec<String>> {
    let packages = workspace
        .packages
        .iter()
        .map(|package| (package.name.as_str(), package))
        .collect::<HashMap<_, _>>();
    let mut features = Vec::new();
    for (package_name, feature_name) in planned_features {
        let package = packages
            .get(package_name)
            .with_context(|| format!("workspace metadata did not include {package_name}"))?;
        if package.features.contains_key(*feature_name) {
            features.push(format!("{package_name}/{feature_name}"));
        }
    }
    Ok(features)
}

fn check_graph(
    label: &str,
    metadata: &Metadata,
    workspace_members: &HashMap<String, String>,
    policy: GraphPolicy,
) -> Result<()> {
    let audit_id = package_id_by_name(metadata, AUDIT_PACKAGE)
        .with_context(|| format!("{label}: failed to find {AUDIT_PACKAGE} package"))?;
    let graph = normal_dependency_graph(metadata);

    if policy.expect_gaze_audit_from_gaze {
        let gaze_id = workspace_members
            .get("gaze")
            .context("workspace metadata did not include gaze")?;
        let Some(path) = path_to_package(gaze_id, &audit_id, &graph) else {
            bail!("{label}: expected gaze feature graph to resolve {AUDIT_PACKAGE}");
        };
        println!(
            "cargo_metadata_audit_isolation: {label}: confirmed {}",
            format_path(&path, metadata)
        );
        return Ok(());
    }

    for (name, id) in workspace_members {
        if name == AUDIT_PACKAGE || AUDIT_RESPONSIBLE_PACKAGES.contains(&name.as_str()) {
            continue;
        }
        if let Some(path) = path_to_package(id, &audit_id, &graph) {
            bail!(
                "{label}: workspace package {name} has a normal dependency path to {AUDIT_PACKAGE}: {}",
                format_path(&path, metadata)
            );
        }
    }

    if policy.safety_net_dep_bans {
        check_safety_net_prohibited_packages(label, metadata, workspace_members, &graph)?;
    }

    println!("cargo_metadata_audit_isolation: {label}: passed");
    Ok(())
}

fn check_safety_net_prohibited_packages(
    label: &str,
    metadata: &Metadata,
    workspace_members: &HashMap<String, String>,
    graph: &HashMap<String, Vec<String>>,
) -> Result<()> {
    for package in SAFETY_NET_PROHIBITED_PACKAGES {
        let Some(package_id) = package_id_by_name(metadata, package) else {
            continue;
        };
        for (workspace_name, workspace_id) in workspace_members {
            let Some(path) = path_to_package(workspace_id, &package_id, graph) else {
                continue;
            };
            if let Some(exemption) = matching_legacy_ner_exemption(package, &path, metadata) {
                println!(
                    "cargo_metadata_audit_isolation: {label}: allowed {} for {workspace_name}: {} ({})",
                    package,
                    format_path(&path, metadata),
                    exemption.reason
                );
                continue;
            }
            bail!(
                "{label}: safety-net graph resolves prohibited package {package} from {workspace_name}: {}",
                format_path(&path, metadata)
            );
        }
    }
    Ok(())
}

fn matching_legacy_ner_exemption<'a>(
    package: &str,
    path: &[String],
    metadata: &Metadata,
) -> Option<&'a LegacyNerExemption> {
    let names = package_names(metadata);
    SAFETY_NET_LEGACY_NER_EXEMPTIONS.iter().find(|exemption| {
        exemption.package == package
            && path.iter().any(|id| {
                names
                    .get(id)
                    .is_some_and(|name| name == exemption.required_path_member)
            })
    })
}

fn cargo_metadata(args: &[&str]) -> Result<Metadata> {
    let output = Command::new("cargo")
        .arg("metadata")
        .arg("--format-version=1")
        .args(args)
        .output()
        .with_context(|| format!("failed to run cargo metadata {}", args.join(" ")))?;
    if !output.status.success() {
        bail!(
            "cargo metadata {} failed:\n{}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata JSON")
}

fn workspace_members_by_name(metadata: &Metadata) -> Result<HashMap<String, String>> {
    let package_names = package_names(metadata);
    let mut members = HashMap::new();
    for id in &metadata.workspace_members {
        let name = package_names
            .get(id)
            .with_context(|| format!("workspace member {id} missing from package list"))?;
        members.insert(name.clone(), id.clone());
    }
    Ok(members)
}

fn package_id_by_name(metadata: &Metadata, name: &str) -> Option<String> {
    metadata
        .packages
        .iter()
        .find(|package| package.name == name)
        .map(|package| package.id.clone())
}

fn package_names(metadata: &Metadata) -> HashMap<String, String> {
    metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package.name.clone()))
        .collect()
}

fn normal_dependency_graph(metadata: &Metadata) -> HashMap<String, Vec<String>> {
    let mut graph = HashMap::new();
    let Some(resolve) = &metadata.resolve else {
        return graph;
    };
    for node in &resolve.nodes {
        let deps = node
            .deps
            .iter()
            .filter(|dep| dep.dep_kinds.iter().any(|kind| kind.kind.is_none()))
            .map(|dep| dep.pkg.clone())
            .collect();
        graph.insert(node.id.clone(), deps);
    }
    graph
}

fn path_to_package(
    start: &str,
    target: &str,
    graph: &HashMap<String, Vec<String>>,
) -> Option<Vec<String>> {
    let mut seen = HashSet::new();
    let mut queue = VecDeque::from([(start.to_string(), vec![start.to_string()])]);
    while let Some((id, path)) = queue.pop_front() {
        if id == target {
            return Some(path);
        }
        if !seen.insert(id.clone()) {
            continue;
        }
        for dep in graph.get(&id).into_iter().flatten() {
            let mut next_path = path.clone();
            next_path.push(dep.clone());
            queue.push_back((dep.clone(), next_path));
        }
    }
    None
}

fn format_path(path: &[String], metadata: &Metadata) -> String {
    let names = package_names(metadata);
    path.iter()
        .map(|id| names.get(id).cloned().unwrap_or_else(|| id.clone()))
        .collect::<Vec<_>>()
        .join(" -> ")
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
    resolve: Option<Resolve>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    #[serde(default)]
    features: HashMap<String, Vec<String>>,
}

#[derive(Debug, Deserialize)]
struct Resolve {
    nodes: Vec<Node>,
}

#[derive(Debug, Deserialize)]
struct Node {
    id: String,
    deps: Vec<NodeDep>,
}

#[derive(Debug, Deserialize)]
struct NodeDep {
    pkg: String,
    dep_kinds: Vec<DepKind>,
}

#[derive(Debug, Deserialize)]
struct DepKind {
    kind: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn safety_net_base_rejects_new_network_clients_from_gaze_types() {
        let graph = graph_category("safety-net-base");
        assert!(graph.policy.safety_net_dep_bans);

        for package in SAFETY_NET_PROHIBITED_PACKAGES {
            let metadata = fixture_metadata(&["gaze-types"], &[("gaze-types", &[*package])]);
            let workspace_members = workspace_members_by_name(&metadata).unwrap();

            let err = check_graph(graph.label, &metadata, &workspace_members, graph.policy)
                .expect_err("safety-net-base must fail closed on new network clients");

            let message = err.to_string();
            assert!(
                message.contains(package),
                "error should name prohibited package {package}: {message}"
            );
            assert!(
                message.contains("gaze-types"),
                "error should trace violation to gaze-types: {message}"
            );
        }
    }

    #[test]
    fn default_graph_does_not_apply_safety_net_network_client_bans() {
        let graph = graph_category("default");
        assert!(!graph.policy.safety_net_dep_bans);

        let metadata = fixture_metadata(&["gaze-types"], &[("gaze-types", &["reqwest"])]);
        let workspace_members = workspace_members_by_name(&metadata).unwrap();

        check_graph(graph.label, &metadata, &workspace_members, graph.policy)
            .expect("default graph must not inherit safety-net-only bans");
    }

    #[test]
    fn legacy_ner_ort_ureq_path_is_the_only_current_safety_net_exemption() {
        let graph = graph_category("safety-net-base");
        let metadata = fixture_metadata(
            &["gaze-recognizers"],
            &[("gaze-recognizers", &["ort"]), ("ort", &["ureq"])],
        );
        let workspace_members = workspace_members_by_name(&metadata).unwrap();

        check_graph(graph.label, &metadata, &workspace_members, graph.policy)
            .expect("legacy NER ort -> ureq path should remain narrowly exempt");
    }

    fn graph_category(label: &str) -> GraphCategory {
        *GRAPH_CATEGORIES
            .iter()
            .find(|graph| graph.label == label)
            .expect("test graph category should exist")
    }

    fn fixture_metadata(workspace_members: &[&str], edges: &[(&str, &[&str])]) -> Metadata {
        let mut names = HashSet::from([AUDIT_PACKAGE.to_string()]);
        for member in workspace_members {
            names.insert((*member).to_string());
        }
        for (from, targets) in edges {
            names.insert((*from).to_string());
            for target in *targets {
                names.insert((*target).to_string());
            }
        }

        let packages = names
            .iter()
            .map(|name| Package {
                id: name.clone(),
                name: name.clone(),
                features: HashMap::new(),
            })
            .collect::<Vec<_>>();
        let nodes = names
            .iter()
            .map(|name| Node {
                id: name.clone(),
                deps: edges
                    .iter()
                    .find(|(from, _)| *from == name)
                    .map(|(_, targets)| {
                        targets
                            .iter()
                            .map(|target| NodeDep {
                                pkg: (*target).to_string(),
                                dep_kinds: vec![DepKind { kind: None }],
                            })
                            .collect()
                    })
                    .unwrap_or_default(),
            })
            .collect::<Vec<_>>();

        Metadata {
            packages,
            workspace_members: workspace_members
                .iter()
                .map(|member| (*member).to_string())
                .collect(),
            resolve: Some(Resolve { nodes }),
        }
    }
}
