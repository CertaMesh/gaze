use std::collections::{HashMap, HashSet, VecDeque};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

const AUDIT_PACKAGE: &str = "gaze-audit";

// Package-level exceptions require an explicit source comment. `gaze-cli` is
// audit-responsible because its audit command reads and purges audit metadata
// through the passive sink crate directly instead of using the `gaze` shim.
const AUDIT_RESPONSIBLE_PACKAGES: &[&str] = &["gaze-cli"];

pub fn run() -> Result<()> {
    let workspace = cargo_metadata(&["--no-deps"])?;
    let workspace_members = workspace_members_by_name(&workspace)?;

    check_graph("default", &cargo_metadata(&[])?, &workspace_members, false)?;
    check_graph(
        "no-default-features",
        &cargo_metadata(&["--no-default-features"])?,
        &workspace_members,
        false,
    )?;
    check_graph(
        "gaze audit feature sanity",
        &cargo_metadata(&["--no-default-features", "--features", "gaze/audit"])?,
        &workspace_members,
        true,
    )?;

    println!("cargo_metadata_audit_isolation: passed");
    Ok(())
}

fn check_graph(
    label: &str,
    metadata: &Metadata,
    workspace_members: &HashMap<String, String>,
    expect_gaze_audit_from_gaze: bool,
) -> Result<()> {
    let audit_id = package_id_by_name(metadata, AUDIT_PACKAGE)
        .with_context(|| format!("{label}: failed to find {AUDIT_PACKAGE} package"))?;
    let graph = normal_dependency_graph(metadata);

    if expect_gaze_audit_from_gaze {
        let gaze_id = workspace_members
            .get("gaze")
            .context("workspace metadata did not include gaze")?;
        let Some(path) = path_to_package(gaze_id, &audit_id, &graph) else {
            bail!("{label}: expected gaze --features audit to resolve {AUDIT_PACKAGE}");
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

    println!("cargo_metadata_audit_isolation: {label}: passed");
    Ok(())
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
