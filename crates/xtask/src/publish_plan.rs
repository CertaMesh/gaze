use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
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
    let output = Command::new("cargo")
        .args(["metadata", "--no-deps", "--format-version", "1"])
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

    for id in &metadata.workspace_members {
        let Some(package) = packages_by_id.get(id).copied() else {
            bail!("publish-plan: workspace member {id} missing from package list");
        };
        if !publishable_ids.contains(&package.id) {
            continue;
        }

        let mut deps = BTreeSet::new();
        for dependency in publish_dependencies(package, &package_ids_by_dir) {
            let dependency = dependency?;
            if dependency == package.id.as_str() {
                continue;
            }
            let dependency_package = packages_by_id
                .get(&dependency)
                .with_context(|| format!("publish-plan: dependency {dependency} missing"))?;
            if !publishable_ids.contains(&dependency) {
                bail!(
                    "publish-plan: published crate {} depends on workspace member {} with publish = false",
                    package.name,
                    dependency_package.name
                );
            }
            deps.insert(dependency);
        }

        for dependency in deps {
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
        let unresolved = indegree
            .into_iter()
            .filter(|(_, degree)| *degree > 0)
            .map(|(id, _)| id)
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
) -> impl Iterator<Item = Result<String>> + 'a {
    package
        .dependencies
        .iter()
        .filter(|dependency| dependency.kind.as_deref() != Some("dev"))
        .filter(|dependency| dependency.source.is_none())
        .filter_map(|dependency| {
            dependency.path.as_ref().map(|path| {
                package_ids_by_dir.get(path).cloned().with_context(|| {
                    format!(
                        "publish-plan: {} has local dependency {} at {}, but no package was found there",
                        package.name,
                        dependency.name,
                        path.display()
                    )
                })
            })
        })
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
            for dependency in publish_dependencies(package, &package_ids_by_dir) {
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

    fn assert_before(positions: &HashMap<&str, usize>, before: &str, after: &str) {
        assert!(
            positions[before] < positions[after],
            "{before} should publish before {after}"
        );
    }
}
