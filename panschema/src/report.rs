//! How `verify` reports what it finds. Every finding goes through a
//! [`Reporter`], which prints the line a person reads to stderr and, with
//! `--message-format json`, one JSON object per finding to stdout, so a
//! script matches a finding by its kind rather than by its wording.

use std::io::{self, Write};

use serde::Serialize;

use crate::findings::Finding;
use crate::import_resolve::LoadWarning;

/// How a run reports its findings.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, clap::ValueEnum)]
pub enum MessageFormat {
    /// Lines for a person, on stderr.
    #[default]
    Human,
    /// The human lines, plus one JSON object per line on stdout.
    Json,
}

/// What a finding does to the run, as the site that reports it knows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Fails the run whenever it is reported.
    Fails,
    /// Fails the run only under `--strict`.
    FailsUnderStrict,
    /// Reported without ever failing the run.
    Warns,
    /// Informational.
    Informs,
}

/// A finding's `level` in the JSON output: what it did to this run.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Level {
    /// It counts against the exit code.
    Error,
    /// It is reported without failing the run.
    Warning,
    /// It is informational.
    Note,
}

impl Effect {
    /// The level a finding with this effect has in a run that is `strict`
    /// or not.
    pub fn level(self, strict: bool) -> Level {
        match self {
            Effect::Fails => Level::Error,
            Effect::FailsUnderStrict if strict => Level::Error,
            Effect::FailsUnderStrict | Effect::Warns => Level::Warning,
            Effect::Informs => Level::Note,
        }
    }

    /// What the human line starts with: the per-file form prints the
    /// violations that fail it bare, as its own list.
    fn prefix(self) -> &'static str {
        match self {
            Effect::Fails => "",
            Effect::FailsUnderStrict | Effect::Warns => "warning: ",
            Effect::Informs => "note: ",
        }
    }
}

/// The file a finding was found in, and whether the human line names it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum File<'a> {
    /// The human line starts with the file.
    Named(&'a str),
    /// Only the JSON object names the file: one data file's own list, or
    /// a schema's load warnings.
    Unnamed(&'a str),
    /// The finding is about no one file, such as an IRI two datasets share.
    None,
}

impl<'a> File<'a> {
    fn path(self) -> Option<&'a str> {
        match self {
            File::Named(path) | File::Unnamed(path) => Some(path),
            File::None => None,
        }
    }
}

/// A finding the command words itself: a note on the `[check.<name>]`
/// policy, or a claim family's count with the reason it went unchecked.
pub struct Text {
    pub kind: &'static str,
    pub message: String,
}

impl Finding for Text {
    fn kind(&self) -> &'static str {
        self.kind
    }
    fn place(&self) -> Option<&str> {
        None
    }
    fn message(&self) -> String {
        self.message.clone()
    }
}

/// One line of the JSON output.
#[derive(Serialize)]
#[serde(tag = "reason", rename_all = "kebab-case")]
enum Line<'a> {
    Finding {
        kind: &'a str,
        level: Level,
        file: Option<&'a str>,
        place: Option<&'a str>,
        entry: Option<&'a str>,
        message: &'a str,
    },
    DatasetRead {
        entry: Option<&'a str>,
        file: &'a str,
        root: Option<&'a str>,
        records: usize,
    },
    Summary {
        entry: Option<&'a str>,
        family: &'a str,
        checked: usize,
        failed: usize,
        uncheckable: usize,
    },
    VerifyFinished {
        success: bool,
        findings: usize,
    },
}

/// A stream whose reader may stop reading before the run ends, as
/// `| head -1` does. What is written after that is dropped, so the run goes
/// on to the exit code its findings earn rather than failing as a command
/// that could not run.
struct UntilClosed<W> {
    inner: W,
    closed: bool,
}

impl<W: Write> UntilClosed<W> {
    /// Run `op` on the stream, or return `done` once the reader has gone.
    fn unless_closed<T>(
        &mut self,
        done: T,
        op: impl FnOnce(&mut W) -> io::Result<T>,
    ) -> io::Result<T> {
        if self.closed {
            return Ok(done);
        }
        match op(&mut self.inner) {
            Err(e) if e.kind() == io::ErrorKind::BrokenPipe => {
                self.closed = true;
                Ok(done)
            }
            outcome => outcome,
        }
    }
}

impl<W: Write> Write for UntilClosed<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.unless_closed(buf.len(), |stream| stream.write(buf))
    }
    fn flush(&mut self) -> io::Result<()> {
        self.unless_closed((), W::flush)
    }
}

/// Reports a run's findings in the format it was asked for, counting them
/// for the closing object.
pub struct Reporter<'w> {
    format: MessageFormat,
    strict: bool,
    entry: Option<String>,
    stderr: Box<dyn Write + 'w>,
    stdout: UntilClosed<Box<dyn Write + 'w>>,
    findings: usize,
    /// Summary objects, held until [`Reporter::finish`] so every one
    /// follows every finding, however many entries a run checks.
    summaries: Vec<u8>,
}

impl Reporter<'static> {
    /// A reporter writing to the process's stderr and stdout.
    pub fn new(format: MessageFormat, strict: bool) -> Self {
        Reporter::to(format, strict, io::stderr(), io::stdout())
    }

    /// A reporter for a command that prints only the human lines, where
    /// strictness, which sets only a JSON finding's level, changes nothing.
    pub fn human() -> Self {
        Reporter::new(MessageFormat::Human, false)
    }
}

impl<'w> Reporter<'w> {
    /// A reporter writing its human lines to `stderr` and its result lines
    /// or JSON objects to `stdout`.
    pub fn to(
        format: MessageFormat,
        strict: bool,
        stderr: impl Write + 'w,
        stdout: impl Write + 'w,
    ) -> Self {
        Reporter {
            format,
            strict,
            entry: None,
            stderr: Box::new(stderr),
            stdout: UntilClosed {
                inner: Box::new(stdout),
                closed: false,
            },
            findings: 0,
            summaries: Vec::new(),
        }
    }

    /// Report a schema load's warnings, each about the file it came from:
    /// one `--strict` refuses with the effect `refused` (fails under
    /// `--strict` for bare `verify`, which checks the schema; warns for a
    /// form that checks data, or a sibling whose own entry counts it), and
    /// every other one as a warning.
    pub fn load_warnings(&mut self, refused: Effect, warnings: &[LoadWarning]) -> io::Result<()> {
        for warning in warnings {
            let effect = if warning.refused_under_strict() {
                refused
            } else {
                Effect::Warns
            };
            let file = warning.file.display().to_string();
            self.finding(effect, File::Unnamed(&file), warning)?;
        }
        Ok(())
    }

    /// The manifest entry the findings that follow belong to.
    pub fn set_entry(&mut self, entry: Option<&str>) {
        self.entry = entry.map(str::to_string);
    }

    /// Report one finding.
    pub fn finding(&mut self, effect: Effect, file: File, finding: &dyn Finding) -> io::Result<()> {
        let message = finding.message();
        match file {
            File::Named(path) => writeln!(self.stderr, "{}{path}: {message}", effect.prefix())?,
            File::Unnamed(_) | File::None => writeln!(self.stderr, "{}{message}", effect.prefix())?,
        }
        self.findings += 1;
        json(
            self.format,
            &mut self.stdout,
            &Line::Finding {
                kind: finding.kind(),
                level: effect.level(self.strict),
                file: file.path(),
                place: finding.place(),
                entry: self.entry.as_deref(),
                message: &message,
            },
        )
    }

    /// Report each of `findings`, all with one effect and file.
    pub fn findings<F: Finding>(
        &mut self,
        effect: Effect,
        file: File,
        findings: &[F],
    ) -> io::Result<()> {
        findings
            .iter()
            .try_for_each(|finding| self.finding(effect, file, finding))
    }

    /// Say, in JSON, which `tree_root` class a data file was read against
    /// and how many records it held. A file read against the wrong root
    /// conforms with no records, and the "conforms" line that tells a person
    /// so is one the JSON form leaves out.
    pub fn dataset_read(
        &mut self,
        file: &str,
        root: Option<&str>,
        records: usize,
    ) -> io::Result<()> {
        json(
            self.format,
            &mut self.stdout,
            &Line::DatasetRead {
                entry: self.entry.as_deref(),
                file,
                root,
                records,
            },
        )
    }

    /// Report a family's counts: the sentence as a note, now, and in JSON a
    /// summary object, written by [`Reporter::finish`] after the findings.
    pub fn summary(
        &mut self,
        sentence: &str,
        family: &str,
        checked: usize,
        failed: usize,
        uncheckable: usize,
    ) -> io::Result<()> {
        writeln!(self.stderr, "{}{sentence}", Effect::Informs.prefix())?;
        json(
            self.format,
            &mut self.summaries,
            &Line::Summary {
                entry: self.entry.as_deref(),
                family,
                checked,
                failed,
                uncheckable,
            },
        )
    }

    /// A line for a person on stdout, such as a file's "conforms", which
    /// the JSON form leaves out so stdout carries JSON alone.
    pub fn result_line(&mut self, line: &str) -> io::Result<()> {
        match self.format {
            MessageFormat::Human => writeln!(self.stdout, "{line}"),
            MessageFormat::Json => Ok(()),
        }
    }

    /// Close a run that checked everything it set out to: in JSON, the
    /// summary objects, then the object saying whether it succeeded and how
    /// many findings it reported.
    pub fn finish(&mut self, success: bool) -> io::Result<()> {
        self.stdout
            .write_all(&std::mem::take(&mut self.summaries))?;
        json(
            self.format,
            &mut self.stdout,
            &Line::VerifyFinished {
                success,
                findings: self.findings,
            },
        )?;
        self.stdout.flush()
    }
}

/// Write `line` as one JSON object on its own line, in the JSON form only.
fn json(format: MessageFormat, out: &mut dyn Write, line: &Line) -> io::Result<()> {
    if format == MessageFormat::Json {
        serde_json::to_writer(&mut *out, line)?;
        writeln!(out)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::findings::kind;
    use serde_json::{Value, json};

    struct Place;
    impl Finding for Place {
        fn kind(&self) -> &'static str {
            kind::REFERENCE_UNRESOLVED
        }
        fn place(&self) -> Option<&str> {
            Some("q1")
        }
        fn message(&self) -> String {
            "q1 names nothing".to_string()
        }
    }

    /// Run `report` against a reporter and return what it wrote to stderr
    /// and to stdout.
    fn run(
        format: MessageFormat,
        strict: bool,
        report: impl FnOnce(&mut Reporter) -> io::Result<()>,
    ) -> (String, String) {
        let (mut stderr, mut stdout) = (Vec::new(), Vec::new());
        {
            let mut reporter = Reporter::to(format, strict, &mut stderr, &mut stdout);
            report(&mut reporter).expect("writes to a Vec");
        }
        (
            String::from_utf8(stderr).unwrap(),
            String::from_utf8(stdout).unwrap(),
        )
    }

    fn lines(stdout: &str) -> Vec<Value> {
        stdout
            .lines()
            .map(|line| serde_json::from_str(line).unwrap_or_else(|e| panic!("{e}: {line}")))
            .collect()
    }

    #[test]
    fn a_findings_level_is_what_it_does_to_the_run() {
        for (effect, strict, level) in [
            (Effect::Fails, false, Level::Error),
            (Effect::Fails, true, Level::Error),
            (Effect::FailsUnderStrict, false, Level::Warning),
            (Effect::FailsUnderStrict, true, Level::Error),
            (Effect::Warns, false, Level::Warning),
            (Effect::Warns, true, Level::Warning),
            (Effect::Informs, false, Level::Note),
            (Effect::Informs, true, Level::Note),
        ] {
            assert_eq!(effect.level(strict), level, "{effect:?}, strict {strict}");
        }
    }

    #[test]
    fn the_human_line_names_the_file_only_when_asked_and_starts_as_the_effect_says() {
        let (stderr, stdout) = run(MessageFormat::Human, false, |r| {
            r.finding(Effect::Fails, File::Named("a.yaml"), &Place)?;
            r.finding(Effect::Fails, File::Unnamed("a.yaml"), &Place)?;
            r.finding(Effect::FailsUnderStrict, File::Named("a.yaml"), &Place)?;
            r.findings(Effect::Warns, File::Unnamed("s.yaml"), &[Place])?;
            r.findings(Effect::Informs, File::None, &[Place])
        });
        assert_eq!(
            stderr,
            "a.yaml: q1 names nothing\n\
             q1 names nothing\n\
             warning: a.yaml: q1 names nothing\n\
             warning: q1 names nothing\n\
             note: q1 names nothing\n"
        );
        assert_eq!(stdout, "", "the human form writes no finding to stdout");
    }

    #[test]
    fn a_json_finding_carries_its_kind_level_file_place_entry_and_message() {
        let (stderr, stdout) = run(MessageFormat::Json, true, |r| {
            r.finding(Effect::FailsUnderStrict, File::Unnamed("a.yaml"), &Place)?;
            r.set_entry(Some("claims"));
            r.finding(Effect::Informs, File::None, &Place)
        });
        assert_eq!(
            stderr, "warning: q1 names nothing\nnote: q1 names nothing\n",
            "the human lines still go to stderr"
        );
        assert_eq!(
            lines(&stdout),
            [
                json!({"reason": "finding", "kind": "reference/unresolved", "level": "error",
                       "file": "a.yaml", "place": "q1", "entry": null,
                       "message": "q1 names nothing"}),
                json!({"reason": "finding", "kind": "reference/unresolved", "level": "note",
                       "file": null, "place": "q1", "entry": "claims",
                       "message": "q1 names nothing"}),
            ]
        );
    }

    #[test]
    fn a_schema_defect_strict_refuses_fails_only_a_strict_run_that_refuses_it() {
        let warnings = [
            LoadWarning::new("s.yaml", kind::SCHEMA_DANGLING_REFERENCE, "range `Nowhere`"),
            LoadWarning::new("i.yaml", kind::SCHEMA_MULTIPLE_TREE_ROOTS, "two roots"),
        ];
        for (strict, refused, dangling) in [
            (false, Effect::Warns, "warning"),
            (true, Effect::Warns, "warning"),
            (false, Effect::FailsUnderStrict, "warning"),
            (true, Effect::FailsUnderStrict, "error"),
        ] {
            let (stderr, stdout) = run(MessageFormat::Json, strict, |r| {
                r.load_warnings(refused, &warnings)
            });
            assert_eq!(
                stderr, "warning: range `Nowhere`\nwarning: two roots\n",
                "the human lines are the same however the run treats them"
            );
            assert_eq!(
                lines(&stdout)
                    .iter()
                    .map(
                        |line| [&line["kind"], &line["level"], &line["file"], &line["place"]]
                            .map(Value::clone)
                    )
                    .collect::<Vec<_>>(),
                [
                    [
                        json!("schema/dangling-reference"),
                        json!(dangling),
                        json!("s.yaml"),
                        Value::Null
                    ],
                    [
                        json!("schema/multiple-tree-roots"),
                        json!("warning"),
                        json!("i.yaml"),
                        Value::Null
                    ],
                ],
                "strict {strict}, a refused defect's effect {refused:?}"
            );
        }
    }

    #[test]
    fn a_summary_prints_its_line_now_and_its_counts_after_every_finding() {
        let report = |r: &mut Reporter| {
            r.set_entry(Some("claims"));
            r.summary("3 of 4 hold", "value-claim", 4, 1, 2)?;
            r.set_entry(Some("other"));
            r.finding(Effect::Informs, File::None, &Place)?;
            r.finish(true)
        };
        let (stderr, stdout) = run(MessageFormat::Human, false, report);
        assert_eq!(
            (stderr.as_str(), stdout.as_str()),
            ("note: 3 of 4 hold\nnote: q1 names nothing\n", "")
        );
        let (stderr, stdout) = run(MessageFormat::Json, false, report);
        assert_eq!(stderr, "note: 3 of 4 hold\nnote: q1 names nothing\n");
        let lines = lines(&stdout);
        let reasons: Vec<_> = lines.iter().map(|line| line["reason"].clone()).collect();
        assert_eq!(
            reasons,
            ["finding", "summary", "verify-finished"],
            "a later entry's finding still comes before an earlier entry's summary"
        );
        assert_eq!(
            lines[1],
            json!({"reason": "summary", "entry": "claims", "family": "value-claim",
                   "checked": 4, "failed": 1, "uncheckable": 2})
        );
    }

    #[test]
    fn the_closing_object_counts_every_finding_and_only_json_writes_one() {
        let report = |r: &mut Reporter| {
            r.finding(Effect::Fails, File::None, &Place)?;
            r.finding(Effect::Informs, File::None, &Place)?;
            r.finish(false)
        };
        let (_, stdout) = run(MessageFormat::Human, false, report);
        assert_eq!(stdout, "");
        let (_, stdout) = run(MessageFormat::Json, false, report);
        assert_eq!(
            lines(&stdout).last(),
            Some(&json!({"reason": "verify-finished", "success": false, "findings": 2}))
        );
    }

    #[test]
    fn a_read_dataset_names_its_root_and_record_count_in_json_alone() {
        let report = |r: &mut Reporter| {
            r.dataset_read("a.yaml", None, 0)?;
            r.set_entry(Some("claims"));
            r.dataset_read("b.yaml", Some("Shelf"), 3)
        };
        assert_eq!(
            run(MessageFormat::Human, false, report),
            (String::new(), String::new())
        );
        let (stderr, stdout) = run(MessageFormat::Json, false, report);
        assert_eq!(stderr, "");
        assert_eq!(
            lines(&stdout),
            [
                json!({"reason": "dataset-read", "entry": null, "file": "a.yaml",
                       "root": null, "records": 0}),
                json!({"reason": "dataset-read", "entry": "claims", "file": "b.yaml",
                       "root": "Shelf", "records": 3}),
            ]
        );
    }

    /// A stdout whose writes or flushes fail with `kind`, counting the
    /// calls that reached it.
    struct Failing {
        kind: io::ErrorKind,
        writes_fail: bool,
        calls: usize,
    }

    impl Write for Failing {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.calls += 1;
            if self.writes_fail {
                Err(self.kind.into())
            } else {
                Ok(buf.len())
            }
        }
        fn flush(&mut self) -> io::Result<()> {
            self.calls += 1;
            Err(self.kind.into())
        }
    }

    /// What `report` returned against a stdout that fails as `stdout`
    /// says, with the stderr it wrote and the calls that reached stdout.
    fn run_against(
        format: MessageFormat,
        mut stdout: Failing,
        report: impl FnOnce(&mut Reporter) -> io::Result<()>,
    ) -> (Result<(), io::ErrorKind>, String, usize) {
        let mut stderr = Vec::new();
        let outcome = {
            let mut reporter = Reporter::to(format, false, &mut stderr, &mut stdout);
            report(&mut reporter).map_err(|e| e.kind())
        };
        (outcome, String::from_utf8(stderr).unwrap(), stdout.calls)
    }

    #[test]
    fn a_reader_that_stops_reading_stdout_ends_the_output_and_not_the_run() {
        let report = |r: &mut Reporter| {
            r.finding(Effect::Fails, File::None, &Place)?;
            r.result_line("a.yaml conforms to s.yaml")?;
            r.finding(Effect::Informs, File::None, &Place)?;
            r.finish(false)
        };
        for format in [MessageFormat::Json, MessageFormat::Human] {
            let closed = Failing {
                kind: io::ErrorKind::BrokenPipe,
                writes_fail: true,
                calls: 0,
            };
            assert_eq!(
                run_against(format, closed, report),
                (
                    Ok(()),
                    "q1 names nothing\nnote: q1 names nothing\n".to_string(),
                    1
                ),
                "{format:?}: the run reports on, stderr is whole, and nothing is written \
                 to stdout after the write that found it closed"
            );
        }
        let closed_at_flush = Failing {
            kind: io::ErrorKind::BrokenPipe,
            writes_fail: false,
            calls: 0,
        };
        let (outcome, _, _) = run_against(MessageFormat::Json, closed_at_flush, report);
        assert_eq!(
            outcome,
            Ok(()),
            "a reader gone by the last flush is no error"
        );
    }

    #[test]
    fn a_stdout_that_fails_any_other_way_stops_the_run() {
        let finding = |r: &mut Reporter| r.finding(Effect::Fails, File::None, &Place);
        let full = |writes_fail| Failing {
            kind: io::ErrorKind::StorageFull,
            writes_fail,
            calls: 0,
        };
        let (outcome, _, _) = run_against(MessageFormat::Json, full(true), finding);
        assert_eq!(outcome, Err(io::ErrorKind::StorageFull), "a failed write");
        let (outcome, _, _) = run_against(MessageFormat::Json, full(false), |r| r.finish(true));
        assert_eq!(outcome, Err(io::ErrorKind::StorageFull), "a failed flush");
    }

    #[test]
    fn a_result_line_reaches_stdout_only_in_the_human_form() {
        let report = |r: &mut Reporter| r.result_line("a.yaml conforms to s.yaml");
        assert_eq!(
            run(MessageFormat::Human, false, report),
            (String::new(), "a.yaml conforms to s.yaml\n".to_string())
        );
        assert_eq!(
            run(MessageFormat::Json, false, report),
            (String::new(), String::new())
        );
    }
}
