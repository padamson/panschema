//! Arbitrary bytes into the LinkML YAML reader: a malformed file is a
//! returned error, never a panic or a hang. Same harness shape as the
//! OWL target — one campaign-long tempdir, skip-on-setup-failure so an
//! environment error can never masquerade as a reader crash.
#![no_main]

use std::path::PathBuf;
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use panschema::io::Reader;

static INPUT: LazyLock<Option<(tempfile::TempDir, PathBuf)>> = LazyLock::new(|| {
    let dir = tempfile::tempdir().ok()?;
    let path = dir.path().join("input.yaml");
    Some((dir, path))
});

fuzz_target!(|data: &[u8]| {
    let Some((_dir, path)) = INPUT.as_ref() else {
        return;
    };
    if std::fs::write(path, data).is_err() {
        return;
    }
    let _ = panschema::yaml_reader::YamlReader::new().read(path);
});
