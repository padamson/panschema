//! `panschema migrate` — emitting a schema's DDL as a versioned migration
//! file a migration runner can discover and apply.
//!
//! The behaviour under test is file identity, determinism, and refusal.
//! The SQL body itself is the Postgres writer's, covered by its own suite.

use std::fs;
use std::path::Path;
use std::process::Command;

use super::write_sample_pkg;

/// The stem a versioned runner discovers: a `V` or `U` prefix, a numeric
/// version, `__`, then a name of word characters only. Mirrors
/// `refinery-core`'s own `STEM_RE`, so a filename this rejects is one the
/// runner would skip silently.
const RUNNER_STEM_RE: &str = r"^([U|V])(\d+(?:\.\d+)?)__(\w+)\.sql$";

fn migrate(dir: &Path, args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_panschema"))
        .arg("migrate")
        .args(args)
        .current_dir(dir)
        .output()
        .expect("failed to execute panschema migrate")
}

/// The only entry in `dir`, as (filename, contents). Panics unless there
/// is exactly one, so a stray second migration fails loudly.
fn sole_migration(dir: &Path) -> (String, String) {
    let mut entries: Vec<_> = fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read migrations dir {}: {e}", dir.display()))
        .map(|e| e.expect("dir entry").path())
        .collect();
    entries.sort();
    assert_eq!(
        entries.len(),
        1,
        "expected exactly one migration in {}; found {entries:?}",
        dir.display()
    );
    let path = &entries[0];
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .expect("utf-8 filename")
        .to_string();
    (name, fs::read_to_string(path).expect("read migration"))
}

/// The initial migration lands as the first version, carries the schema's
/// DDL, and the command says where it went.
#[test]
fn migrate_writes_the_first_versioned_migration_and_reports_its_path() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = write_sample_pkg(tmp.path(), "sample-pkg");
    let migrations = tmp.path().join("migrations");

    let out = migrate(
        tmp.path(),
        &[
            "--schema",
            pkg.join("sample_schema.yaml").to_str().unwrap(),
            "--migrations",
            migrations.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (name, body) = sole_migration(&migrations);
    assert_eq!(
        name, "V1__sample_schema.sql",
        "initial migration should be version 1 named for the schema"
    );
    assert!(
        body.contains("CREATE TABLE") && body.contains("person"),
        "migration should carry the schema's DDL; got:\n{body}"
    );

    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        stdout.contains("V1__sample_schema.sql"),
        "migrate should report the path it wrote; stdout was:\n{stdout}"
    );
}

/// The emitted name is one the runner's discovery regex actually matches.
#[test]
fn the_migration_filename_is_one_a_versioned_runner_discovers() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = write_sample_pkg(tmp.path(), "sample-pkg");
    let migrations = tmp.path().join("migrations");

    let out = migrate(
        tmp.path(),
        &[
            "--schema",
            pkg.join("sample_schema.yaml").to_str().unwrap(),
            "--migrations",
            migrations.to_str().unwrap(),
        ],
    );
    assert!(out.status.success(), "migrate failed");

    let (name, _) = sole_migration(&migrations);
    let re = regex::Regex::new(RUNNER_STEM_RE).expect("compile runner stem regex");
    let caps = re
        .captures(&name)
        .unwrap_or_else(|| panic!("`{name}` is not a filename a versioned runner discovers"));
    assert_eq!(
        &caps[1], "V",
        "the initial migration is versioned, not out-of-order"
    );
    assert_eq!(&caps[2], "1", "the initial migration is the first version");
}

/// A runner checksums the raw SQL text, so the same schema must produce the
/// same bytes on every machine and every panschema build. A timestamp or a
/// tool-version banner in the body would change the checksum on upgrade and
/// abort the whole run.
#[test]
fn the_emitted_sql_is_byte_identical_across_runs() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = write_sample_pkg(tmp.path(), "sample-pkg");
    let schema = pkg.join("sample_schema.yaml");

    let mut bodies = Vec::new();
    for run in ["first", "second"] {
        let migrations = tmp.path().join(format!("migrations-{run}"));
        let out = migrate(
            tmp.path(),
            &[
                "--schema",
                schema.to_str().unwrap(),
                "--migrations",
                migrations.to_str().unwrap(),
            ],
        );
        assert!(out.status.success(), "migrate {run} run failed");
        bodies.push(sole_migration(&migrations));
    }

    assert_eq!(
        bodies[0], bodies[1],
        "two runs from the same schema produced different migrations"
    );

    let body = &bodies[0].1;
    assert!(
        !body.contains(env!("CARGO_PKG_VERSION")),
        "migration carries panschema's version, so every upgrade rewrites the \
         checksum; got:\n{body}"
    );
    assert!(
        !body.contains("@generated by panschema v"),
        "migration carries the generated-by version banner; got:\n{body}"
    );
}

/// Re-running against an unchanged schema is a no-op. It must not append a
/// second copy, and must not rewrite the first — an applied migration is
/// immutable to the byte.
#[test]
fn rerunning_against_an_unchanged_schema_writes_nothing_and_says_so() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = write_sample_pkg(tmp.path(), "sample-pkg");
    let schema = pkg.join("sample_schema.yaml");
    let migrations = tmp.path().join("migrations");
    let args = [
        "--schema",
        schema.to_str().unwrap(),
        "--migrations",
        migrations.to_str().unwrap(),
    ];

    let first = migrate(tmp.path(), &args);
    assert!(first.status.success(), "first migrate failed");
    let before = sole_migration(&migrations);

    let second = migrate(tmp.path(), &args);
    assert!(
        second.status.success(),
        "re-running against an unchanged schema should succeed as a no-op: {}",
        String::from_utf8_lossy(&second.stderr)
    );

    let after = sole_migration(&migrations);
    assert_eq!(
        before, after,
        "re-run rewrote or duplicated the existing migration"
    );

    let stdout = String::from_utf8_lossy(&second.stdout);
    assert!(
        stdout.contains("already"),
        "re-run should report the migration already exists; stdout was:\n{stdout}"
    );
}

/// Emitting into a directory that already holds migrations this schema did
/// not produce is refused, not guessed at. Incremental emission is a later
/// slice; until then the honest answer is to stop.
#[test]
fn migrating_into_a_non_empty_directory_refuses() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let pkg = write_sample_pkg(tmp.path(), "sample-pkg");
    let migrations = tmp.path().join("migrations");
    fs::create_dir_all(&migrations).expect("mkdir migrations");
    let hand_written = migrations.join("V1__hand_written.sql");
    fs::write(&hand_written, "CREATE TABLE legacy ();\n").expect("write existing migration");

    let out = migrate(
        tmp.path(),
        &[
            "--schema",
            pkg.join("sample_schema.yaml").to_str().unwrap(),
            "--migrations",
            migrations.to_str().unwrap(),
        ],
    );
    assert!(
        !out.status.success(),
        "migrate should refuse a directory that already contains migrations"
    );

    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        stderr.contains("not empty") || stderr.contains("already contains"),
        "refusal should say the directory is not empty; stderr was:\n{stderr}"
    );
    assert_eq!(
        fs::read_to_string(&hand_written).expect("read existing migration"),
        "CREATE TABLE legacy ();\n",
        "refusal must leave the existing migration untouched"
    );
    let entries: Vec<_> = fs::read_dir(&migrations)
        .expect("read migrations dir")
        .map(|e| e.expect("dir entry").file_name())
        .collect();
    assert_eq!(
        entries.len(),
        1,
        "refusal must not write a file; found {entries:?}"
    );
}

/// A manifest can declare where a schema's migrations live, so a
/// manifest-driven run emits them the same way it emits every other output.
#[test]
fn manifest_driven_migrate_emits_for_each_configured_schema() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let consumer = tmp.path();
    write_sample_pkg(consumer, "sample-pkg");

    fs::write(
        consumer.join("panschema.toml"),
        r#"
[schemas]
sample_schema = { path = "./sample-pkg" }

[generate.sample_schema]
postgres = "out/schema.sql"
migrations = "db/migrations/"
"#,
    )
    .expect("write manifest");

    let out = migrate(consumer, &[]);
    assert!(
        out.status.success(),
        "manifest-driven migrate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (name, body) = sole_migration(&consumer.join("db/migrations"));
    assert_eq!(name, "V1__sample_schema.sql");
    assert!(
        body.contains("CREATE TABLE"),
        "manifest-driven migration should carry DDL; got:\n{body}"
    );
}

/// A schema sharing one `id` slot across its record classes splits
/// reference entities from per-dataset records through `slot_usage`. Both
/// halves must reach the emitted DDL: the scoped class keys on its own
/// column, and the class that did not override keeps the shared identifier.
#[test]
fn a_slot_usage_key_becomes_the_primary_key_in_the_emitted_migration() {
    let tmp = tempfile::tempdir().expect("tempdir");
    let migrations = tmp.path().join("migrations");
    // The command runs with its cwd in the tempdir, so the fixture needs an
    // absolute path.
    let schema = Path::new("tests/fixtures/shared_id_scoping.yaml")
        .canonicalize()
        .expect("fixture exists");

    let out = migrate(
        tmp.path(),
        &[
            "--schema",
            schema.to_str().unwrap(),
            "--migrations",
            migrations.to_str().unwrap(),
        ],
    );
    assert!(
        out.status.success(),
        "migrate failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let (_, body) = sole_migration(&migrations);
    assert!(
        body.contains(r#""id" text PRIMARY KEY"#),
        "the class whose slot_usage sets `key: true` should key on that column, \
         not on a synthetic surrogate; got:\n{body}"
    );
    assert!(
        !body.contains("gen_random_uuid"),
        "no class here needs a surrogate key; got:\n{body}"
    );
}

/// The command writes files. Someone reaching for it needs to know from
/// `--help` alone that it will not touch their database.
#[test]
fn migrate_help_says_it_never_connects_to_a_database() {
    let out = Command::new(env!("CARGO_BIN_EXE_panschema"))
        .args(["migrate", "--help"])
        .output()
        .expect("failed to execute panschema migrate --help");
    assert!(out.status.success(), "migrate --help failed");

    let help = String::from_utf8_lossy(&out.stdout).to_lowercase();
    assert!(
        help.contains("never connects") || help.contains("does not connect"),
        "--help must state the command never connects to a database; got:\n{help}"
    );
    assert!(
        help.contains("writes"),
        "--help must state the command writes files; got:\n{help}"
    );
}
