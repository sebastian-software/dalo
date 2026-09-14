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

fn fixture_source(document: &str) -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/fixture");
    fs::create_dir_all(&package).unwrap();
    fs::write(package.join("PLUGIN.toml"), document).unwrap();
    fs::write(package.join("handler.mjs"), "process.exit(0);\n").unwrap();
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

#[test]
fn schema_fixtures_use_a_real_json_schema_2020_12_engine() {
    let schema: serde_json::Value =
        serde_json::from_str(include_str!("../docs/spec/plugin-v1.schema.json")).unwrap();
    let validator = jsonschema::validator_for(&schema).expect("schema must compile");
    let fixtures = [
        ("valid", true, true),
        ("unknown-field", false, false),
        ("unknown-tool-field", false, false),
        ("missing-description", false, false),
        ("wrong-tool-version", false, false),
        ("semantic-missing-tool", true, false),
        ("bad-binding", true, false),
    ];
    for (name, schema_valid, parser_valid) in fixtures {
        let document = fs::read_to_string(format!(
            "{}/tests/fixtures/package-schema/{name}.toml",
            env!("CARGO_MANIFEST_DIR")
        ))
        .unwrap();
        let value: toml::Value = toml::from_str(&document).unwrap();
        let value = serde_json::to_value(value).unwrap();
        assert_eq!(
            validator.is_valid(&value),
            schema_valid,
            "schema fixture {name}"
        );
        let temp = fixture_source(&document);
        let inventory = plugin::scan_source_plugins("fixture", temp.path());
        assert_eq!(
            !inventory.plugins.is_empty(),
            parser_valid,
            "production parser fixture {name}: {:?}",
            inventory.warnings
        );
    }
}

#[test]
fn author_validation_reports_reference_scope_and_external_availability() {
    let local =
        dalo::package_validation::validate_with_source_id("example", &example_root()).unwrap();
    assert!(local.valid);
    assert_eq!(local.packages[0].source_id, "example");
    assert!(
        local
            .members
            .iter()
            .all(|member| member.status == "resolved")
    );

    let external_document = manifest().replace(
        "ref = \"skill:design-review\"\nrequirement = \"required\"",
        "ref = \"skill:other:design-review\"\nrequirement = \"required\"",
    );
    let temp = source(&external_document);
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert!(!report.valid);
    assert_eq!(report.members[0].status, "unavailable_external");
    assert!(report.members[0].detail.contains("without a store"));

    let optional_document = external_document.replace(
        "ref = \"skill:other:design-review\"\nrequirement = \"required\"",
        "ref = \"skill:other:design-review\"\nrequirement = \"optional\"",
    ) + "\n[[plugin.requires]]\nref = \"plugin:other:optional\"\nrequirement = \"optional\"\n";
    let optional_temp = source(&optional_document);
    let optional_report = dalo::package_validation::validate(optional_temp.path()).unwrap();
    assert!(optional_report.valid);
    assert_eq!(optional_report.members[0].status, "unavailable_external");
    assert_eq!(
        optional_report.dependencies[0].status,
        "unavailable_external"
    );
}

#[test]
fn required_dependency_cycles_are_reported_by_the_production_graph_resolver() {
    let temp = tempfile::tempdir().unwrap();
    for (name, dependency) in [("alpha", "beta"), ("beta", "alpha")] {
        let package = temp.path().join(format!("plugins/{name}"));
        fs::create_dir_all(&package).unwrap();
        fs::write(
            package.join("PLUGIN.toml"),
            format!(
                "schema_version = 1\n[plugin]\nname = \"{name}\"\ndescription = \"cycle\"\n\n[[plugin.requires]]\nref = \"plugin:{dependency}\"\nrequirement = \"required\"\n"
            ),
        )
        .unwrap();
    }
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert!(!report.valid);
    assert!(
        report
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.code == "required_dependency_cycle")
    );
}

#[test]
fn malformed_or_non_markdown_instructions_do_not_resolve() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/example");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("PLUGIN.toml"),
        "schema_version = 1\n[plugin]\nname = \"example\"\ndescription = \"instructions\"\n\n[[plugin.members]]\nref = \"instruction:style\"\nrequirement = \"recommended\"\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("instructions")).unwrap();
    fs::write(
        temp.path().join("instructions/style.txt"),
        "wrong extension",
    )
    .unwrap();
    fs::write(temp.path().join("instructions/style.md"), [0xff, 0xfe]).unwrap();
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert_eq!(report.members[0].status, "missing");
    assert!(!report.diagnostics.iter().any(|diagnostic| {
        diagnostic
            .message
            .contains("resolved in the supplied source tree")
    }));
}

#[cfg(unix)]
#[test]
fn symlinked_instruction_roots_do_not_resolve_references() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/example");
    let real_instructions = temp.path().join("real-instructions");
    fs::create_dir_all(&package).unwrap();
    fs::create_dir_all(&real_instructions).unwrap();
    fs::write(real_instructions.join("style.md"), "Use the house style.").unwrap();
    fs::write(
        package.join("PLUGIN.toml"),
        "schema_version = 1\n[plugin]\nname = \"example\"\ndescription = \"instructions\"\n\n[[plugin.members]]\nref = \"instruction:style\"\nrequirement = \"recommended\"\n",
    )
    .unwrap();
    std::os::unix::fs::symlink(&real_instructions, temp.path().join("instructions")).unwrap();
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert_eq!(report.members[0].status, "missing");
}

#[test]
fn oversized_instruction_files_do_not_resolve_references() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/example");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("PLUGIN.toml"),
        "schema_version = 1\n[plugin]\nname = \"example\"\ndescription = \"instructions\"\n\n[[plugin.members]]\nref = \"instruction:style\"\nrequirement = \"recommended\"\n",
    )
    .unwrap();
    fs::create_dir_all(temp.path().join("instructions")).unwrap();
    fs::write(
        temp.path().join("instructions/style.md"),
        vec![b'x'; 1024 * 1024 + 1],
    )
    .unwrap();
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert_eq!(report.members[0].status, "missing");
}

#[test]
fn ambiguous_slot_and_stable_id_references_are_reported() {
    let temp = tempfile::tempdir().unwrap();
    let package = temp.path().join("plugins/example");
    fs::create_dir_all(&package).unwrap();
    fs::write(
        package.join("PLUGIN.toml"),
        "schema_version = 1\n[plugin]\nname = \"example\"\ndescription = \"ambiguous\"\n\n[[plugin.members]]\nref = \"skill:beta\"\nrequirement = \"required\"\n",
    )
    .unwrap();
    for (slot, id) in [("alpha", "beta"), ("beta", "other")] {
        let skill = temp.path().join(format!("skills/{slot}"));
        fs::create_dir_all(&skill).unwrap();
        fs::write(
            skill.join("SKILL.md"),
            format!("---\nname: {slot}\nid: {id}\ndescription: Fixture\n---\nBody\n"),
        )
        .unwrap();
    }
    let report = dalo::package_validation::validate(temp.path()).unwrap();
    assert!(!report.valid);
    assert_eq!(report.members[0].status, "ambiguous");
    assert!(
        report.members[0]
            .detail
            .contains("multiple local components")
    );
}
