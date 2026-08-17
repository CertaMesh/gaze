use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use serde::Deserialize;

pub fn run() -> Result<()> {
    for package in build_publish_plan(&cargo_metadata()?)? {
        println!("{}", package.name);
    }
    Ok(())
}

fn cargo_metadata() -> Result<Metadata> {
    cargo_metadata_at(Path::new("."))
}

fn cargo_metadata_at(root: &Path) -> Result<Metadata> {
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
        .current_dir(root)
        .output()
        .context("failed to run cargo metadata")?;
    if !output.status.success() {
        bail!(
            "cargo metadata failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }
    serde_json::from_slice(&output.stdout).context("failed to parse cargo metadata")
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct WorkspaceMember {
    pub name: String,
    pub manifest_dir: PathBuf,
}

pub(crate) fn workspace_members(root: &Path) -> Result<Vec<WorkspaceMember>> {
    workspace_members_from_metadata(&cargo_metadata_at(root)?)
}

fn workspace_members_from_metadata(metadata: &Metadata) -> Result<Vec<WorkspaceMember>> {
    let packages_by_id = metadata
        .packages
        .iter()
        .map(|package| (package.id.as_str(), package))
        .collect::<HashMap<_, _>>();
    let mut members = metadata
        .workspace_members
        .iter()
        .map(|id| {
            let package = packages_by_id
                .get(id.as_str())
                .with_context(|| format!("workspace member {id} missing from cargo metadata"))?;
            let manifest_dir = package
                .manifest_path
                .parent()
                .with_context(|| format!("package {} manifest has no parent", package.name))?;
            Ok(WorkspaceMember {
                name: package.name.clone(),
                manifest_dir: manifest_dir.to_path_buf(),
            })
        })
        .collect::<Result<Vec<_>>>()?;
    members.sort_by(|left, right| left.name.cmp(&right.name));
    Ok(members)
}

fn build_publish_plan(metadata: &Metadata) -> Result<Vec<PublishPackage>> {
    let packages_by_id: HashMap<String, &Package> = metadata
        .packages
        .iter()
        .map(|package| (package.id.clone(), package))
        .collect();
    let package_ids_by_dir = package_ids_by_dir(metadata)?;
    let workspace_order: HashMap<String, usize> = metadata
        .workspace_members
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), index))
        .collect();
    let publishable_ids: HashSet<String> = metadata
        .workspace_members
        .iter()
        .filter_map(|id| packages_by_id.get(id).copied())
        .filter(|package| is_publishable(package))
        .map(|package| package.id.clone())
        .collect();

    let mut indegree: HashMap<String, usize> = publishable_ids
        .iter()
        .cloned()
        .map(|id| (id, 0_usize))
        .collect();
    let mut dependents: HashMap<String, Vec<String>> = HashMap::new();
    let mut dependency_edges_by_package: HashMap<String, BTreeMap<String, PublishDependencyKind>> =
        HashMap::new();

    for id in &metadata.workspace_members {
        let Some(package) = packages_by_id.get(id).copied() else {
            bail!("publish-plan: workspace member {id} missing from package list");
        };
        if !publishable_ids.contains(&package.id) {
            continue;
        }

        let mut deps: BTreeMap<String, PublishDependencyKind> = BTreeMap::new();
        for dependency in publish_dependencies(package, &package_ids_by_dir) {
            let dependency = dependency?;
            if dependency.package_id == package.id.as_str() {
                continue;
            }
            let dependency_package =
                packages_by_id
                    .get(&dependency.package_id)
                    .with_context(|| {
                        format!("publish-plan: dependency {} missing", dependency.package_id)
                    })?;
            if !publishable_ids.contains(&dependency.package_id) {
                bail!(
                    "publish-plan: published crate {} depends on workspace member {} with publish = false",
                    package.name,
                    dependency_package.name
                );
            }
            deps.entry(dependency.package_id)
                .and_modify(|kind| *kind = kind.merge(dependency.kind))
                .or_insert(dependency.kind);
        }

        dependency_edges_by_package.insert(package.id.clone(), deps.clone());

        for dependency in deps.into_keys() {
            dependents
                .entry(dependency)
                .or_default()
                .push(package.id.clone());
            *indegree
                .get_mut(&package.id)
                .expect("publishable package has indegree") += 1;
        }
    }

    let mut ready: BTreeSet<(usize, String)> = indegree
        .iter()
        .filter(|(_, degree)| **degree == 0)
        .map(|(id, _)| {
            (
                *workspace_order
                    .get(id)
                    .expect("publishable package has workspace order"),
                id.clone(),
            )
        })
        .collect();
    let mut ordered = Vec::with_capacity(publishable_ids.len());

    while let Some((_, id)) = ready.pop_first() {
        let package = packages_by_id
            .get(&id)
            .copied()
            .expect("ready package exists");
        ordered.push(PublishPackage {
            name: package.name.clone(),
        });

        if let Some(next_packages) = dependents.get(&id) {
            for next in next_packages {
                let degree = indegree
                    .get_mut(next)
                    .expect("dependent publishable package has indegree");
                *degree -= 1;
                if *degree == 0 {
                    ready.insert((
                        *workspace_order
                            .get(next)
                            .expect("dependent package has workspace order"),
                        next.clone(),
                    ));
                }
            }
        }
    }

    if ordered.len() != publishable_ids.len() {
        let unresolved_ids = indegree
            .iter()
            .filter(|(_, degree)| **degree > 0)
            .map(|(id, _)| id.clone())
            .collect::<HashSet<_>>();
        if let Some(cycle) = find_dependency_cycle(&dependency_edges_by_package, &unresolved_ids) {
            let description = format_dependency_cycle(&cycle, &packages_by_id);
            if cycle
                .iter()
                .any(|edge| edge.kind == PublishDependencyKind::VersionedDev)
            {
                bail!(
                    "publish-plan: versioned dev-dependency cycle blocks crates.io publish order: {description}. Break the test-only dependency cycle or publish one crate manually first."
                );
            }
            bail!("publish-plan: workspace publish dependencies contain a cycle: {description}");
        }

        let unresolved = unresolved_ids
            .into_iter()
            .filter_map(|id| packages_by_id.get(&id).map(|package| package.name.as_str()))
            .collect::<Vec<_>>()
            .join(", ");
        bail!("publish-plan: workspace publish dependencies contain a cycle: {unresolved}");
    }

    Ok(ordered)
}

fn package_ids_by_dir(metadata: &Metadata) -> Result<HashMap<PathBuf, String>> {
    let mut by_dir = HashMap::new();
    for package in &metadata.packages {
        let package_dir = package
            .manifest_path
            .parent()
            .with_context(|| format!("{}: Cargo.toml has no parent directory", package.name))?;
        by_dir.insert(package_dir.to_path_buf(), package.id.clone());
    }
    Ok(by_dir)
}

fn publish_dependencies<'a>(
    package: &'a Package,
    package_ids_by_dir: &'a HashMap<PathBuf, String>,
) -> impl Iterator<Item = Result<PublishDependency>> + 'a {
    package
        .dependencies
        .iter()
        .filter_map(|dependency| match publish_dependency_kind(dependency) {
            Some(kind) => dependency.path.as_ref().map(|path| {
                package_ids_by_dir
                    .get(path)
                    .cloned()
                    .map(|package_id| PublishDependency { package_id, kind })
                    .with_context(|| {
                        format!(
                            "publish-plan: {} has local dependency {} at {}, but no package was found there",
                            package.name,
                            dependency.name,
                            path.display()
                        )
                    })
            }),
            None => None,
        })
}

fn publish_dependency_kind(dependency: &Dependency) -> Option<PublishDependencyKind> {
    if dependency.source.is_some() || dependency.path.is_none() {
        return None;
    }
    if dependency.kind.as_deref() == Some("dev") {
        return versioned_dev_dependency(dependency).then_some(PublishDependencyKind::VersionedDev);
    }
    Some(PublishDependencyKind::Normal)
}

fn versioned_dev_dependency(dependency: &Dependency) -> bool {
    dependency.req.trim() != "*"
}

fn is_publishable(package: &Package) -> bool {
    package
        .publish
        .as_ref()
        .is_none_or(|registries| !registries.is_empty())
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishPackage {
    name: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct PublishDependency {
    package_id: String,
    kind: PublishDependencyKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PublishDependencyKind {
    Normal,
    VersionedDev,
}

impl PublishDependencyKind {
    fn merge(self, other: Self) -> Self {
        if self == Self::Normal || other == Self::Normal {
            Self::Normal
        } else {
            Self::VersionedDev
        }
    }

    fn cycle_label(self) -> &'static str {
        match self {
            Self::Normal => "dependency",
            Self::VersionedDev => "versioned dev-dependency",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct DependencyCycleEdge {
    from: String,
    to: String,
    kind: PublishDependencyKind,
}

fn find_dependency_cycle(
    edges_by_package: &HashMap<String, BTreeMap<String, PublishDependencyKind>>,
    unresolved_ids: &HashSet<String>,
) -> Option<Vec<DependencyCycleEdge>> {
    let mut visited = HashSet::new();
    let mut stack = Vec::new();
    let mut in_stack = HashMap::new();

    let mut sorted_unresolved_ids = unresolved_ids.iter().collect::<Vec<_>>();
    sorted_unresolved_ids.sort();

    for id in sorted_unresolved_ids {
        if visited.contains(id) {
            continue;
        }
        if let Some(cycle) = visit_dependency_cycle(
            id,
            edges_by_package,
            unresolved_ids,
            &mut visited,
            &mut stack,
            &mut in_stack,
        ) {
            return Some(cycle);
        }
    }

    None
}

fn visit_dependency_cycle(
    id: &str,
    edges_by_package: &HashMap<String, BTreeMap<String, PublishDependencyKind>>,
    unresolved_ids: &HashSet<String>,
    visited: &mut HashSet<String>,
    stack: &mut Vec<String>,
    in_stack: &mut HashMap<String, usize>,
) -> Option<Vec<DependencyCycleEdge>> {
    visited.insert(id.to_string());
    in_stack.insert(id.to_string(), stack.len());
    stack.push(id.to_string());

    if let Some(edges) = edges_by_package.get(id) {
        for (dependency, kind) in edges {
            if !unresolved_ids.contains(dependency) {
                continue;
            }
            if let Some(start) = in_stack.get(dependency).copied() {
                let mut cycle = Vec::new();
                for pair in stack[start..].windows(2) {
                    let from = pair[0].clone();
                    let to = pair[1].clone();
                    let kind = edges_by_package[&from][&to];
                    cycle.push(DependencyCycleEdge { from, to, kind });
                }
                cycle.push(DependencyCycleEdge {
                    from: stack.last().expect("cycle stack is non-empty").clone(),
                    to: dependency.clone(),
                    kind: *kind,
                });
                return Some(cycle);
            }
            if !visited.contains(dependency) {
                if let Some(cycle) = visit_dependency_cycle(
                    dependency,
                    edges_by_package,
                    unresolved_ids,
                    visited,
                    stack,
                    in_stack,
                ) {
                    return Some(cycle);
                }
            }
        }
    }

    let removed = stack.pop().expect("cycle stack is non-empty");
    in_stack.remove(&removed);
    None
}

fn format_dependency_cycle(
    cycle: &[DependencyCycleEdge],
    packages_by_id: &HashMap<String, &Package>,
) -> String {
    let Some(first) = cycle.first() else {
        return "<empty cycle>".to_string();
    };
    let mut description = package_name(&first.from, packages_by_id).to_string();
    for edge in cycle {
        description.push_str(" --");
        description.push_str(edge.kind.cycle_label());
        description.push_str("--> ");
        description.push_str(package_name(&edge.to, packages_by_id));
    }
    description
}

fn package_name<'a>(id: &'a str, packages_by_id: &'a HashMap<String, &Package>) -> &'a str {
    packages_by_id
        .get(id)
        .map(|package| package.name.as_str())
        .unwrap_or(id)
}

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
    workspace_members: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct Package {
    id: String,
    name: String,
    manifest_path: PathBuf,
    publish: Option<Vec<String>>,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
    req: String,
    path: Option<PathBuf>,
    source: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn workspace_publish_plan_is_topological_and_keeps_cli_last() -> Result<()> {
        let metadata = cargo_metadata()?;
        let plan = build_publish_plan(&metadata)?;
        let positions: HashMap<&str, usize> = plan
            .iter()
            .enumerate()
            .map(|(index, package)| (package.name.as_str(), index))
            .collect();

        assert_eq!(
            plan.last().map(|package| package.name.as_str()),
            Some("gaze-cli")
        );
        assert!(positions.contains_key("gaze-mcp-bridge"));
        assert!(positions.contains_key("gaze-token-bridge"));

        assert_before(&positions, "gaze-pii", "gaze-cli");
        assert_before(&positions, "gaze-audit", "gaze-pii");
        assert_before(&positions, "gaze-mcp-bridge", "gaze-cli");
        assert_before(&positions, "gaze-token-bridge", "gaze-cli");

        let package_ids_by_dir = package_ids_by_dir(&metadata)?;
        let packages_by_id: HashMap<&str, &Package> = metadata
            .packages
            .iter()
            .map(|package| (package.id.as_str(), package))
            .collect();
        let package_names_by_id: HashMap<&str, &str> = metadata
            .packages
            .iter()
            .map(|package| (package.id.as_str(), package.name.as_str()))
            .collect();
        let publishable_ids: HashSet<&str> = metadata
            .workspace_members
            .iter()
            .filter_map(|id| packages_by_id.get(id.as_str()).copied())
            .filter(|package| is_publishable(package))
            .map(|package| package.id.as_str())
            .collect();

        for id in &publishable_ids {
            let package = packages_by_id[id];
            for dependency in metadata_publish_order_dependencies(package, &package_ids_by_dir) {
                let dependency = dependency?;
                if dependency == package.id.as_str()
                    || !publishable_ids.contains(dependency.as_str())
                {
                    continue;
                }
                let dependency_name = package_names_by_id[dependency.as_str()];
                assert_before(&positions, dependency_name, &package.name);
            }
        }

        Ok(())
    }

    #[test]
    fn unversioned_dev_dependency_does_not_force_publish_order() -> Result<()> {
        let metadata = synthetic_metadata([
            synthetic_package(
                "crate-a",
                [synthetic_dependency("crate-b", Some("dev"), "*")],
            ),
            synthetic_package("crate-b", []),
        ]);

        let plan = build_publish_plan(&metadata)?;

        assert_eq!(
            plan.iter()
                .map(|package| package.name.as_str())
                .collect::<Vec<_>>(),
            ["crate-a", "crate-b"]
        );
        Ok(())
    }

    #[test]
    fn versioned_dev_dependency_cycle_names_the_cycle() {
        let metadata = synthetic_metadata([
            synthetic_package(
                "crate-a",
                [synthetic_dependency("crate-b", Some("dev"), "^1.0.0")],
            ),
            synthetic_package("crate-b", [synthetic_dependency("crate-a", None, "^1.0.0")]),
        ]);

        let error = build_publish_plan(&metadata).unwrap_err().to_string();

        assert!(error.contains("versioned dev-dependency cycle"));
        assert!(
            error.contains("crate-a --versioned dev-dependency--> crate-b --dependency--> crate-a")
        );
    }

    fn assert_before(positions: &HashMap<&str, usize>, before: &str, after: &str) {
        assert!(
            positions[before] < positions[after],
            "{before} should publish before {after}"
        );
    }

    fn metadata_publish_order_dependencies<'a>(
        package: &'a Package,
        package_ids_by_dir: &'a HashMap<PathBuf, String>,
    ) -> impl Iterator<Item = Result<String>> + 'a {
        package.dependencies.iter().filter_map(|dependency| {
            if dependency.source.is_some() {
                return None;
            }
            if dependency.kind.as_deref() == Some("dev") && dependency.req.trim() == "*" {
                return None;
            }
            dependency.path.as_ref().map(|path| {
                package_ids_by_dir.get(path).cloned().with_context(|| {
                    format!(
                        "test metadata dependency {} at {} did not resolve",
                        dependency.name,
                        path.display()
                    )
                })
            })
        })
    }

    fn synthetic_metadata<const N: usize>(packages: [Package; N]) -> Metadata {
        Metadata {
            workspace_members: packages.iter().map(|package| package.id.clone()).collect(),
            packages: packages.into(),
        }
    }

    fn synthetic_package<const N: usize>(name: &str, dependencies: [Dependency; N]) -> Package {
        Package {
            id: synthetic_package_id(name),
            name: name.to_string(),
            manifest_path: synthetic_package_dir(name).join("Cargo.toml"),
            publish: None,
            dependencies: dependencies.into(),
        }
    }

    fn synthetic_dependency(name: &str, kind: Option<&str>, req: &str) -> Dependency {
        Dependency {
            name: name.to_string(),
            kind: kind.map(str::to_string),
            req: req.to_string(),
            path: Some(synthetic_package_dir(name)),
            source: None,
        }
    }

    fn synthetic_package_id(name: &str) -> String {
        format!("path+file:///workspace/{name}#{name}@1.0.0")
    }

    fn synthetic_package_dir(name: &str) -> PathBuf {
        PathBuf::from(format!("/workspace/{name}"))
    }
}
