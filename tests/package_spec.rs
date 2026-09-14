//! Author-facing conformance examples use the production parser without a store.

use std::fs;
use std::path::{Path, PathBuf};

use dalo::hook::{self, HookProvider};
use dalo::plugin;

fn example_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/packages/source")
}

fn manifest() -> String {
    fs::read_to_string(example_root().join("plugins/design-review/PLUGIN.toml")).unwrap()
}

fn source(document: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/design-review");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("PLUGIN.toml"), document).unwrap();
    // A validator must only read this file, never run it.
    fs::write(
        package.join("context.mjs"),
        "throw new Error('Validation must not execute this handler');\n",
    )
    .unwrap();
    temp
}

#[test]
fn complete_author_example_has_valid_local_contracts_for_both_providers() {
    let inventory = plugin::scan_source_plugins("example", &example_root());
    assert!(inventory.warnings.is_empty(), "{:?}", inventory.warnings);
    assert_eq!(inventory.plugins.len(), 1);
    let package = &inventory.plugins[0];
    assert_eq!(package.members.len(), 2);
    assert_eq!(package.tools.len(), 1);
    assert_eq!(package.hooks.len(), 1);
    for provider in [HookProvider::Claude, HookProvider::Codex] {
        assert!(hook::provider_supports_descriptor(
            provider,
            &package.hooks[0].descriptor
        ));
    }
    assert!(
        example_root()
            .join("skills/design-review/SKILL.md")
            .is_file()
    );
    assert!(
        example_root()
            .join("instructions/design-context.md")
            .is_file()
    );
}

#[test]
fn invalid_author_contracts_never_become_partially_valid_packages() {
    let original = manifest();
    let cases = [
        ("schema_version = 1", "schema_version = 2"),
        ("name = \"design-review\"", "name = \"wrong-directory\""),
        ("cwd = \"tool_root\"", "cwd = \"project\""),
        ("entry = \"context.mjs\"", "entry = \"../context.mjs\""),
        ("entry = \"context.mjs\"", "entry = \"missing.mjs\""),
        ("tool = \"context\"", "tool = \"missing-tool\""),
        ("effect = \"add_context\"", "effect = \"replace_output\""),
        ("phase = \"before\"", "phase = \"start\""),
        ("type = \"path\"", "type = \"boolean\""),
        ("field = \"session.cwd\"", "field = \"transcript.path\""),
        (
            "requirement = \"required\"",
            "requirement = \"recommended\"",
        ),
        ("retry = \"never\"", "retry = \"always\""),
        ("timeout_ms = 2000", "timeout_ms = 99"),
    ];
    for (from, to) in cases {
        assert!(original.contains(from));
        let temp = source(&original.replace(from, to));
        let inventory = plugin::scan_source_plugins("example", temp.path());
        assert!(inventory.plugins.is_empty(), "accepted {to}");
        assert!(
            !inventory.warnings.is_empty(),
            "missing diagnostic for {to}"
        );
    }
    let temp = source(&format!("installer = \"run-me.sh\"\n{original}"));
    let inventory = plugin::scan_source_plugins("example", temp.path());
    assert!(inventory.plugins.is_empty());
    assert!(!inventory.warnings.is_empty());
}

#[test]
fn each_descriptor_version_is_checked_independently() {
    let original: toml::Value = toml::from_str(&manifest()).unwrap();
    for component in ["package", "tool", "hook"] {
        let mut document = original.clone();
        let descriptor = match component {
            "package" => &mut document,
            name => &mut document[name][0],
        };
        descriptor["schema_version"] = toml::Value::Integer(2);
        let temp = source(&toml::to_string(&document).unwrap());
        let inventory = plugin::scan_source_plugins("example", temp.path());
        assert!(
            inventory.plugins.is_empty(),
            "accepted {component} version 2"
        );
        assert!(!inventory.warnings.is_empty());
    }
}

#[test]
fn optional_hooks_need_an_explicit_omission_and_cannot_weaken_required_hooks() {
    let original = manifest();
    // Change only the hook requirement, not skill or tool requirements.
    let (package, descriptor) = original.split_once("[[hook]]").unwrap();
    let optional = format!(
        "{package}[[hook]]{}",
        descriptor.replace("requirement = \"required\"", "requirement = \"optional\"")
    );
    let without_fallback = source(&optional);
    assert!(
        plugin::scan_source_plugins("example", without_fallback.path())
            .plugins
            .is_empty()
    );

    let optional = optional.replace(
        "retry = \"never\"",
        "retry = \"never\"\nfallback = \"omit\"",
    );
    let with_fallback = source(&optional);
    let inventory = plugin::scan_source_plugins("example", with_fallback.path());
    assert!(inventory.warnings.is_empty());
    assert_eq!(inventory.plugins.len(), 1);

    let required = source(&original.replace(
        "retry = \"never\"",
        "retry = \"never\"\nfallback = \"omit\"",
    ));
    assert!(
        plugin::scan_source_plugins("example", required.path())
            .plugins
            .is_empty()
    );
}

#[test]
fn validation_does_not_resolve_members_or_create_runtime_state() {
    let temp = source(&manifest());
    let before = fs::read(temp.path().join("plugins/design-review/context.mjs")).unwrap();
    let inventory = plugin::scan_source_plugins("example", temp.path());
    // A local contract can be valid while its external members are missing.
    // Keep that limit explicit in the validator and specification.
    assert!(inventory.warnings.is_empty());
    assert_eq!(inventory.plugins.len(), 1);
    assert!(!temp.path().join("skills").exists());
    assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    assert_eq!(
        fs::read(temp.path().join("plugins/design-review/context.mjs")).unwrap(),
        before
    );
}
