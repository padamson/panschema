//! Arbitrary bytes into the OWL/Turtle reader: a malformed file is a
//! returned error, never a panic or a hang. The reader's public surface
//! takes a path, so inputs go through a real file — one tempdir for the
//! whole campaign, one fixed path rewritten per exec. An environment
//! failure (disk full, fd exhaustion) skips the exec instead of
//! panicking, because a panic here would be recorded as a crash artifact
//! against a perfectly innocent input and send triage hunting a reader
//! bug that does not exist.
#![no_main]

use std::path::PathBuf;
use std::sync::LazyLock;

use libfuzzer_sys::fuzz_target;
use panschema::io::Reader;

static INPUT: LazyLock<Option<(tempfile::TempDir, PathBuf)>> = LazyLock::new(|| {
    let dir = tempfile::tempdir().ok()?;
    let path = dir.path().join("input.ttl");
    Some((dir, path))
});

fuzz_target!(|data: &[u8]| {
    let Some((_dir, path)) = INPUT.as_ref() else {
        return;
    };
    if std::fs::write(path, data).is_err() {
        return;
    }
    // Ok or Err are both fine; the contract under fuzz is only that the
    // reader neither panics nor hangs.
    let _ = panschema::owl_reader::OwlReader::new().read(path);
});
