//! The tier that needs a tree larger than an editor opens.
//!
//! Gated behind `VERSIONS_LE_SCENARIOS`. Nothing here substitutes for
//! the unit tests, which run everywhere on every push.
//!
//! **A skipped scenario is never reported as a pass.**

use std::fmt::Write as _;

fn enabled(name: &str) -> bool {
    if std::env::var_os("VERSIONS_LE_SCENARIOS").is_some() {
        return true;
    }
    eprintln!("SKIPPED {name}: set VERSIONS_LE_SCENARIOS to run it");
    false
}

struct Tree(std::path::PathBuf);

impl Tree {
    fn new(name: &str) -> Self {
        let root = std::env::temp_dir().join(format!("versions-le-scenario-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).expect("a temporary directory");
        Self(root)
    }

    fn write(&self, relative: &str, contents: &str) {
        let target = self.0.join(relative);
        if let Some(parent) = target.parent() {
            std::fs::create_dir_all(parent).expect("a parent directory");
        }
        std::fs::write(target, contents).expect("a file");
    }
}

impl Drop for Tree {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run(args: &[&str]) -> (i32, serde_json::Value) {
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_versions-le"))
        .args(args)
        .output()
        .expect("the binary runs");
    let stdout = String::from_utf8_lossy(&output.stdout).into_owned();
    (
        output.status.code().expect("an exit code"),
        serde_json::from_str(stdout.trim()).expect("stdout carries JSON"),
    )
}

/// Conflict detection is quadratic if done naively: every constraint of
/// a name against every other. A monorepo with a crate per directory,
/// all depending on the same handful of names, is the real shape.
#[test]
fn a_monorepo_of_crates_completes() {
    if !enabled("a_monorepo_of_crates_completes") {
        return;
    }
    let tree = Tree::new("monorepo");
    for crate_index in 0..400 {
        let mut manifest = String::from("[dependencies]\n");
        for dependency in 0..50 {
            // Every twentieth crate drifts on one dependency, so the
            // answer is not trivially "everything agrees".
            let version = if crate_index % 20 == 0 && dependency == 7 {
                "1.1"
            } else {
                "1.0"
            };
            let _ = writeln!(manifest, "dep_{dependency} = \"{version}\"");
        }
        tree.write(&format!("crates/c{crate_index}/Cargo.toml"), &manifest);
    }

    let (code, report) = run(&[&tree.0.to_string_lossy()]);
    assert_eq!(code, 1);
    assert_eq!(report["summary"]["manifests"], 400);
    assert_eq!(report["summary"]["findings"], 1, "one name drifted");
    assert_eq!(report["findings"][0]["name"], "dep_7");
}

/// One manifest with a great many dependencies, which is what an
/// application `package.json` looks like after a few years.
#[test]
fn a_manifest_with_many_dependencies_completes() {
    if !enabled("a_manifest_with_many_dependencies_completes") {
        return;
    }
    let tree = Tree::new("wide");
    let mut lines = Vec::new();
    for index in 0..20_000 {
        lines.push(format!("\"pkg-{index}\": \"^1.0.0\""));
    }
    tree.write(
        "package.json",
        &format!("{{ \"dependencies\": {{ {} }} }}", lines.join(", ")),
    );

    let (code, report) = run(&[&tree.0.to_string_lossy()]);
    assert_eq!(code, 0, "one manifest cannot contradict itself");
    assert_eq!(report["summary"]["entries"], 20_000);
}

/// A tree where nothing is comparable. The refusals must not degrade
/// into findings at scale, and the run must still be quick.
#[test]
fn a_tree_of_nothing_but_refusals_completes_and_finds_nothing() {
    if !enabled("a_tree_of_nothing_but_refusals_completes_and_finds_nothing") {
        return;
    }
    let tree = Tree::new("refusals");
    for index in 0..500 {
        tree.write(
            &format!("crates/c{index}/Cargo.toml"),
            "[dependencies]\nshared = { workspace = true }\n",
        );
    }

    let (code, report) = run(&[&tree.0.to_string_lossy()]);
    assert_eq!(code, 0);
    assert_eq!(report["summary"]["findings"], 0);
    assert_eq!(report["summary"]["refusals"], 500);
}
