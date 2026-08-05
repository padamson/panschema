//! The shipped agent-facing skill is held true by these tests rather than by
//! discipline.
//!
//! Two mechanisms, with deliberately different reach. The manifest example is
//! **extracted from the reference file and executed**, so a documented example
//! that stops working fails here. And each of the code's own enumerations —
//! output formats, CLI subcommands, `[generate]` keys — asserts that the
//! reference mentions every entry, so a new capability cannot ship
//! undocumented. Neither checks that the surrounding prose is *accurate*; that
//! residue is stated in the skill itself.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("workspace root")
        .to_path_buf()
}

fn skill_ref(name: &str) -> String {
    let path = repo_root().join("skills/panschema/references").join(name);
    fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()))
}

/// The first fenced ```toml block in `markdown`.
fn first_toml_block(markdown: &str) -> String {
    let start = markdown
        .find("```toml")
        .expect("the reference must carry a fenced toml example");
    let after = &markdown[start + "```toml".len()..];
    let end = after.find("```").expect("unterminated toml fence");
    after[..end].trim_start_matches('\n').to_string()
}

fn panschema_bin() -> PathBuf {
    // The integration-test binary sits alongside the CLI binary.
    let mut p = std::env::current_exe().expect("test exe");
    p.pop();
    if p.ends_with("deps") {
        p.pop();
    }
    p.join(if cfg!(windows) {
        "panschema.exe"
    } else {
        "panschema"
    })
}

/// The manifest reference's example, run for real: build a package around it,
/// invoke `generate`, and require the artifacts it promises.
#[test]
fn the_documented_manifest_example_actually_generates() {
    let example = first_toml_block(&skill_ref("manifest.md"));
    assert!(
        example.contains("[schemas.") && example.contains("[generate."),
        "the example must show both tables — a lone [generate] block is the \
         documented no-op trap; got:\n{example}"
    );

    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    fs::create_dir_all(root.join("schema")).expect("schema dir");
    fs::write(
        root.join("panschema-publish.toml"),
        "[schema]\nname = \"demo\"\nversion = \"0.1.0\"\nlinkml = \"1.7.0\"\n\n\
         [files]\nmain = \"schema/demo.yaml\"\n",
    )
    .expect("publish toml");
    fs::write(
        root.join("schema/demo.yaml"),
        "id: https://example.org/demo\nname: demo\ndefault_range: string\n\
         classes:\n  Thing:\n    attributes:\n      id: {identifier: true}\n",
    )
    .expect("schema");
    fs::write(root.join("panschema.toml"), &example).expect("manifest");

    let out = Command::new(panschema_bin())
        .arg("generate")
        .current_dir(root)
        .output()
        .expect("run generate");
    assert!(
        out.status.success(),
        "the documented manifest example must generate cleanly.\nstdout: {}\nstderr: {}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );

    // Every output path the example declares must exist afterwards.
    let declared: Vec<&str> = example
        .lines()
        .filter_map(|l| l.split_once('='))
        .map(|(_, v)| v.trim().trim_matches('"'))
        .filter(|v| v.contains('/') && !v.starts_with('.'))
        .collect();
    assert!(
        !declared.is_empty(),
        "the example should declare at least one output path"
    );
    for rel in declared {
        assert!(
            root.join(rel).exists(),
            "example declares `{rel}` but generation produced no such file; \
             stderr: {}",
            String::from_utf8_lossy(&out.stderr)
        );
    }
}

#[test]
fn every_output_format_is_documented() {
    let doc = skill_ref("formats.md");
    for id in panschema::io::FormatRegistry::with_defaults().writer_format_ids() {
        assert!(
            doc.contains(id),
            "format `{id}` is registered but absent from the formats reference"
        );
    }
}

#[test]
fn every_generate_manifest_key_is_documented() {
    let doc = skill_ref("manifest.md");
    // The serialized field names are the source of truth for what a manifest
    // may contain; a renamed or added key must reach the reference.
    for key in panschema::manifest::GenerateConfig::key_names() {
        assert!(
            doc.contains(key.as_str()),
            "`[generate]` key `{key}` is accepted by the parser but absent \
             from the manifest reference"
        );
    }
}

#[test]
fn every_subcommand_is_documented() {
    let doc = skill_ref("cli.md");
    for sub in [
        "generate",
        "validate",
        "migrate",
        "publish",
        "serve",
        "init",
        "add",
        "fetch",
        "verify",
        "release",
        "completions",
        "styleguide",
    ] {
        assert!(
            doc.contains(sub),
            "subcommand `{sub}` is absent from the CLI reference"
        );
    }
}

/// The skill's own entry point must point at each reference, or the depth is
/// unreachable in a session that only loads SKILL.md.
#[test]
fn the_skill_links_every_reference() {
    let skill =
        fs::read_to_string(repo_root().join("skills/panschema/SKILL.md")).expect("SKILL.md");
    for name in ["cli.md", "manifest.md", "formats.md"] {
        assert!(skill.contains(name), "SKILL.md must link references/{name}");
    }
    let refs = repo_root().join("skills/panschema/references");
    for entry in fs::read_dir(&refs).expect("references dir") {
        let file = entry.expect("dir entry").file_name();
        let name = file.to_string_lossy();
        assert!(
            skill.contains(name.as_ref()),
            "references/{name} exists but SKILL.md never links it"
        );
    }
    let _ = Path::new("");
}

/// The plugin is how consumers install and update the skill, so its declared
/// version is what they pin against. Drift from the crate version would ship
/// a plugin claiming to be a release it isn't.
#[test]
fn the_plugin_manifest_version_tracks_the_crate_version() {
    let manifest = fs::read_to_string(repo_root().join(".claude-plugin/plugin.json"))
        .expect("read plugin.json");
    let plugin_version = manifest
        .lines()
        .find_map(|l| l.trim().strip_prefix("\"version\":"))
        .map(|v| v.trim().trim_matches(|c| c == '"' || c == ',').to_string())
        .expect("plugin.json declares a version");

    let cargo = fs::read_to_string(repo_root().join("panschema/Cargo.toml")).expect("read Cargo");
    let crate_version = cargo
        .lines()
        .find_map(|l| l.strip_prefix("version = "))
        .map(|v| v.trim().trim_matches('"').to_string())
        .expect("Cargo.toml declares a version");

    assert_eq!(
        plugin_version, crate_version,
        "the plugin version must track the crate version — bump both together"
    );
}

/// The skill has to live where the plugin ships it from. A copy left under
/// `.claude/skills/` would be project-local (visible only when working *in*
/// panschema) and would drift from the distributed one.
#[test]
fn the_skill_ships_from_the_plugin_directory_only() {
    assert!(
        repo_root().join("skills/panschema/SKILL.md").is_file(),
        "the plugin's skill directory holds the skill"
    );
    assert!(
        !repo_root().join(".claude/skills/panschema").exists(),
        "and no project-local duplicate shadows it"
    );
}
