//! The tools an edition builds with: mdbook and its plugins, pinned per
//! tag in `<book>/toolchain.toml` and installed once per pin.
//!
//! A released edition builds with exactly the versions its tag names, so
//! it renders the way it did when it shipped however the repo's tools
//! have moved on since. The unreleased edition may name branch tips, so
//! the dogfood loop survives, or nothing at all, in which case the tools
//! come from `PATH`.

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

/// The file in a book directory naming the tools that build it.
pub const TOOLCHAIN_FILENAME: &str = "toolchain.toml";

/// `<book>/toolchain.toml`: the mdbook and plugin versions a tag's book
/// builds with.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolchainFile {
    /// The mdbook version. Unset, mdbook comes from `PATH`, which only
    /// the unreleased edition may do.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mdbook: Option<String>,
    /// The preprocessors and other plugins, in the order their assets
    /// are installed.
    #[serde(default, rename = "plugin", skip_serializing_if = "Vec::is_empty")]
    pub plugins: Vec<PluginPin>,
}

/// One `[[plugin]]` in `toolchain.toml`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PluginPin {
    /// The binary's name, which is also how it is run and, for a
    /// crates.io pin, the crate.
    pub name: String,
    /// A crates.io version.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    /// A git repository to install from, with `rev` or `branch`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub git: Option<String>,
    /// An exact commit in `git`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub rev: Option<String>,
    /// A branch in `git`; only the unreleased edition may build from one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
    /// The arguments that generate the plugin's book assets, run in the
    /// book directory before the build. Empty means nothing to install.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub install: Vec<String>,
}

/// Why a tool could not be made ready or run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToolError {
    /// `cargo install` of `tool` at `pin` failed, or a branch's tip could
    /// not be resolved.
    Install {
        tool: String,
        pin: String,
        output: String,
    },
    /// The tool ran and failed; `output` is what it printed.
    Run { output: String },
}

/// The tools an edition builds with. [`InstalledToolchain`] installs the
/// pinned versions; tests substitute one that runs in-process.
pub trait Toolchain {
    /// Run `plugin` with `args` in `book_dir`, at the version `pins`
    /// names it.
    fn install_assets(
        &self,
        pins: &ToolchainFile,
        plugin: &str,
        args: &[String],
        book_dir: &Path,
    ) -> Result<(), ToolError>;
    /// Build the book in `book_dir` into `dest` with the mdbook `pins`
    /// names, returning the directory holding the rendered HTML: `dest`
    /// itself, or a subdirectory of it when the book renders through more
    /// than one backend.
    fn build(
        &self,
        pins: &ToolchainFile,
        book_dir: &Path,
        dest: &Path,
    ) -> Result<PathBuf, ToolError>;
}

/// One process a toolchain runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invocation {
    pub program: PathBuf,
    /// Arguments as the OS sees them, so a path with bytes that are not
    /// UTF-8 reaches the tool unchanged.
    pub args: Vec<OsString>,
    /// Where to run it; the caller's directory when unset.
    pub cwd: Option<PathBuf>,
    /// Directories put ahead of `PATH` for this process, so mdbook finds
    /// the pinned plugins by name.
    pub path_prepend: Vec<PathBuf>,
}

/// Runs the processes a toolchain needs. [`ProcessRunner`] spawns them;
/// tests substitute one that records them.
pub trait Runner {
    /// Run `invocation`, returning its standard output, or its whole
    /// output when it cannot start or exits non-zero.
    fn run(&self, invocation: &Invocation) -> Result<String, String>;
}

/// Spawns each process as asked.
#[derive(Debug, Default, Clone, Copy)]
pub struct ProcessRunner;

impl Runner for ProcessRunner {
    fn run(&self, invocation: &Invocation) -> Result<String, String> {
        let program = invocation.program.display();
        let mut command = Command::new(&invocation.program);
        command.args(&invocation.args);
        if let Some(cwd) = &invocation.cwd {
            command.current_dir(cwd);
        }
        if !invocation.path_prepend.is_empty() {
            let mut paths = invocation.path_prepend.clone();
            if let Some(inherited) = std::env::var_os("PATH") {
                paths.extend(std::env::split_paths(&inherited));
            }
            let path = std::env::join_paths(paths)
                .map_err(|e| format!("could not build a PATH for `{program}`: {e}"))?;
            command.env("PATH", path);
        }
        let output = command
            .output()
            .map_err(|e| format!("could not run `{program}`: {e}"))?;
        if output.status.success() {
            return Ok(String::from_utf8_lossy(&output.stdout).into_owned());
        }
        Err(format!(
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        ))
    }
}

/// mdbook and the plugins at the versions a toolchain file names, each
/// installed once under `root/<tool>-<pin>/` and reused by every later
/// edition and run that names the same pin. Without a pin, a tool comes
/// from `PATH`.
pub struct InstalledToolchain {
    root: PathBuf,
    runner: Box<dyn Runner>,
    /// Branch tips resolved in this run, so a branch named twice is
    /// looked up once and both uses build the same commit.
    tips: RefCell<HashMap<String, String>>,
}

impl InstalledToolchain {
    /// Tools installed under `root`, run by `runner`.
    pub fn with_runner(root: impl Into<PathBuf>, runner: Box<dyn Runner>) -> Self {
        InstalledToolchain {
            root: root.into(),
            runner,
            tips: RefCell::new(HashMap::new()),
        }
    }

    /// Tools installed under `root`, spawned as real processes.
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self::with_runner(root, Box::new(ProcessRunner))
    }

    /// Tools installed under panschema's cache directory.
    pub fn in_cache() -> Result<Self, crate::cache::CacheError> {
        Ok(Self::new(crate::cache::cache_root()?.join("toolchains")))
    }
}

/// A tool made ready: its binary, or its bare name when it comes from
/// `PATH`, and the directory holding it when it was installed.
struct Tool {
    program: PathBuf,
    bin_dir: Option<PathBuf>,
}

/// Every tool a toolchain file names, ready to run.
struct Prepared {
    mdbook: Tool,
    plugins: Vec<(String, Tool)>,
}

impl InstalledToolchain {
    fn prepare(&self, pins: &ToolchainFile) -> Result<Prepared, ToolError> {
        let mdbook = match &pins.mdbook {
            None => from_path("mdbook"),
            Some(version) => self.ensure("mdbook", version, version, &["--version", version])?,
        };
        let mut plugins = Vec::new();
        for pin in &pins.plugins {
            plugins.push((pin.name.clone(), self.plugin(pin)?));
        }
        Ok(Prepared { mdbook, plugins })
    }

    fn plugin(&self, pin: &PluginPin) -> Result<Tool, ToolError> {
        let name = &pin.name;
        match (&pin.version, &pin.git) {
            (Some(version), _) => self.ensure(name, version, version, &["--version", version]),
            (None, Some(url)) => {
                // Anything but a commit id moves, so it is resolved to the
                // commit it names today and installed under that.
                let (label, rev) = match (&pin.rev, &pin.branch) {
                    (Some(rev), _) if is_commit_id(rev) => (format!("{url}#{rev}"), rev.clone()),
                    (Some(rev), _) => {
                        let label = format!("{url}#{rev}");
                        let tip = self.tip(name, url, rev, &label)?;
                        (label, tip)
                    }
                    (None, Some(branch)) => {
                        let label = format!("{url}#{branch}");
                        let tip = self.tip(name, url, &format!("refs/heads/{branch}"), &label)?;
                        (label, tip)
                    }
                    (None, None) => return Ok(from_path(name)),
                };
                self.ensure(name, &label, &rev, &["--git", url, "--rev", &rev])
            }
            (None, None) => Ok(from_path(name)),
        }
    }

    /// `tool` at `pin`, installed under `<root>/<tool>-<key>` with
    /// `cargo install` and `source` (`--version` or `--git`/`--rev`)
    /// unless its binary is already there.
    fn ensure(&self, tool: &str, pin: &str, key: &str, source: &[&str]) -> Result<Tool, ToolError> {
        let dir = self.root.join(format!("{tool}-{key}"));
        let bin_dir = dir.join("bin");
        let program = bin_dir.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX));
        if !program.is_file() {
            // A crates.io install names the crate first; a git install
            // names it last, after the source.
            let mut args: Vec<OsString> = vec!["install".into()];
            let by_version = source.first() == Some(&"--version");
            if by_version {
                args.push(tool.into());
            }
            args.extend(source.iter().map(OsString::from));
            args.push("--locked".into());
            args.push("--root".into());
            args.push(dir.clone().into_os_string());
            if !by_version {
                args.push(tool.into());
            }
            self.runner
                .run(&Invocation {
                    program: PathBuf::from("cargo"),
                    args,
                    cwd: None,
                    path_prepend: Vec::new(),
                })
                .map_err(|output| ToolError::Install {
                    tool: tool.to_string(),
                    pin: pin.to_string(),
                    output,
                })?;
        }
        Ok(Tool {
            program,
            bin_dir: Some(bin_dir),
        })
    }

    /// The commit `pattern` (a `refs/heads/` branch, a tag, any ref name
    /// `git ls-remote` matches) names in `url`, looked up once per run.
    fn tip(&self, tool: &str, url: &str, pattern: &str, pin: &str) -> Result<String, ToolError> {
        if let Some(tip) = self.tips.borrow().get(pin) {
            return Ok(tip.clone());
        }
        let install_error = |output: String| ToolError::Install {
            tool: tool.to_string(),
            pin: pin.to_string(),
            output,
        };
        let listing = self
            .runner
            .run(&Invocation {
                program: PathBuf::from("git"),
                args: vec!["ls-remote".into(), url.into(), pattern.into()],
                cwd: None,
                path_prepend: Vec::new(),
            })
            .map_err(install_error)?;
        let tip = listing
            .split_whitespace()
            .next()
            .filter(|tip| !tip.is_empty())
            .ok_or_else(|| install_error(format!("`{url}` has no ref matching `{pattern}`")))?
            .to_string();
        self.tips.borrow_mut().insert(pin.to_string(), tip.clone());
        Ok(tip)
    }
}

/// Whether `rev` is a full commit id, the one spelling of a git rev that
/// cannot move.
pub(crate) fn is_commit_id(rev: &str) -> bool {
    rev.len() == 40 && rev.bytes().all(|b| b.is_ascii_hexdigit())
}

fn from_path(tool: &str) -> Tool {
    Tool {
        program: PathBuf::from(tool),
        bin_dir: None,
    }
}

impl Toolchain for InstalledToolchain {
    fn install_assets(
        &self,
        pins: &ToolchainFile,
        plugin: &str,
        args: &[String],
        book_dir: &Path,
    ) -> Result<(), ToolError> {
        // Only this plugin is made ready here; mdbook and the rest wait
        // for the build, so a failure is reported under the tool it is
        // about.
        let tool = match pins.plugins.iter().find(|p| p.name == plugin) {
            Some(pin) => self.plugin(pin)?.program,
            None => PathBuf::from(plugin),
        };
        self.runner
            .run(&Invocation {
                program: tool,
                args: args.iter().map(OsString::from).collect(),
                cwd: Some(book_dir.to_path_buf()),
                path_prepend: Vec::new(),
            })
            .map(|_| ())
            .map_err(|output| ToolError::Run { output })
    }

    fn build(
        &self,
        pins: &ToolchainFile,
        book_dir: &Path,
        dest: &Path,
    ) -> Result<PathBuf, ToolError> {
        let prepared = self.prepare(pins)?;
        // mdbook finds each preprocessor by name, so the pinned ones lead
        // its PATH.
        let path_prepend = prepared
            .plugins
            .iter()
            .filter_map(|(_, tool)| tool.bin_dir.clone())
            .collect();
        self.runner
            .run(&Invocation {
                program: prepared.mdbook.program,
                args: vec![
                    "build".into(),
                    book_dir.as_os_str().to_os_string(),
                    "--dest-dir".into(),
                    dest.as_os_str().to_os_string(),
                ],
                cwd: None,
                path_prepend,
            })
            .map_err(|output| ToolError::Run { output })?;
        // mdbook just read this file, so it is there and parses.
        let book_toml = std::fs::read_to_string(book_dir.join("book.toml")).unwrap_or_default();
        Ok(html_root(&book_toml, dest))
    }
}

/// Where mdbook put the HTML for a book configured by `book_toml`:
/// `html/` under the destination when several `[output.*]` tables
/// declare several backends, the destination itself otherwise (one
/// table, or none, which means the HTML backend alone).
fn html_root(book_toml: &str, dest: &Path) -> PathBuf {
    let backends = toml::from_str::<toml::Value>(book_toml)
        .ok()
        .and_then(|book| book.get("output")?.as_table().map(|outputs| outputs.len()))
        .unwrap_or(0);
    if backends > 1 {
        dest.join("html")
    } else {
        dest.to_path_buf()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;
    use std::fs;

    /// Records every process and stands in for `cargo install` by creating
    /// the binary the install would leave behind, so the "already
    /// installed" check sees it next time. `git ls-remote` answers with a
    /// fixed tip. A tool named `broken` fails to install.
    #[derive(Default)]
    struct FakeRunner {
        calls: RefCell<Vec<Invocation>>,
    }

    impl Runner for FakeRunner {
        fn run(&self, invocation: &Invocation) -> Result<String, String> {
            self.calls.borrow_mut().push(invocation.clone());
            let args = strs(&invocation.args);
            if invocation.program == Path::new("git")
                && args.first().map(String::as_str) == Some("ls-remote")
            {
                return Ok(format!("{}\trefs/heads/{}\n", "a".repeat(40), args[2]));
            }
            if invocation.program == Path::new("cargo") {
                let root = args
                    .iter()
                    .position(|a| a == "--root")
                    .map(|i| &args[i + 1])
                    .unwrap();
                let tool = args
                    .iter()
                    .position(|a| a == "install")
                    .map(|i| &args[i + 1])
                    .unwrap();
                let tool = if tool.starts_with("--") {
                    // `cargo install --git <url> ... <crate>`: the crate is the last argument.
                    args.last().unwrap()
                } else {
                    tool
                };
                if tool == "broken" {
                    return Err("error: failed to compile `broken`".to_string());
                }
                let bin = Path::new(root).join("bin");
                fs::create_dir_all(&bin).unwrap();
                fs::write(
                    bin.join(format!("{tool}{}", std::env::consts::EXE_SUFFIX)),
                    "",
                )
                .unwrap();
            }
            Ok(String::new())
        }
    }

    fn pins(mdbook: Option<&str>, plugins: &[PluginPin]) -> ToolchainFile {
        ToolchainFile {
            mdbook: mdbook.map(str::to_string),
            plugins: plugins.to_vec(),
        }
    }

    fn versioned(name: &str, version: &str) -> PluginPin {
        PluginPin {
            name: name.to_string(),
            version: Some(version.to_string()),
            git: None,
            rev: None,
            branch: None,
            install: vec!["install".to_string()],
        }
    }

    fn from_git(name: &str, rev: Option<&str>, branch: Option<&str>) -> PluginPin {
        PluginPin {
            name: name.to_string(),
            version: None,
            git: Some("https://example.com/repo".to_string()),
            rev: rev.map(str::to_string),
            branch: branch.map(str::to_string),
            install: vec!["install".to_string()],
        }
    }

    fn toolchain() -> (tempfile::TempDir, InstalledToolchain, &'static FakeRunner) {
        let dir = tempfile::tempdir().unwrap();
        let runner: &'static FakeRunner = Box::leak(Box::default());
        let toolchain = InstalledToolchain::with_runner(
            dir.path().join("toolchains"),
            Box::new(Leaked(runner)),
        );
        (dir, toolchain, runner)
    }

    /// Lets a test keep a handle on the runner it hands over.
    struct Leaked(&'static FakeRunner);

    impl Runner for Leaked {
        fn run(&self, invocation: &Invocation) -> Result<String, String> {
            self.0.run(invocation)
        }
    }

    fn strs(args: &[OsString]) -> Vec<String> {
        args.iter()
            .map(|a| a.to_string_lossy().into_owned())
            .collect()
    }

    fn installs(runner: &FakeRunner) -> Vec<Vec<String>> {
        runner
            .calls
            .borrow()
            .iter()
            .filter(|c| c.program == Path::new("cargo"))
            .map(|c| strs(&c.args))
            .collect()
    }

    fn exe(name: &str) -> String {
        format!("{name}{}", std::env::consts::EXE_SUFFIX)
    }

    #[test]
    fn a_pinned_tool_installs_into_its_own_directory_once() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();
        let pins = pins(Some("0.5.2"), &[versioned("mdbook-listings", "0.2.2")]);

        toolchain
            .build(&pins, &book, &dir.path().join("out1"))
            .unwrap();
        toolchain
            .build(&pins, &book, &dir.path().join("out2"))
            .unwrap();

        let expected_mdbook = vec![
            "install",
            "mdbook",
            "--version",
            "0.5.2",
            "--locked",
            "--root",
            &root.join("mdbook-0.5.2").to_string_lossy(),
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        let expected_plugin = vec![
            "install",
            "mdbook-listings",
            "--version",
            "0.2.2",
            "--locked",
            "--root",
            &root.join("mdbook-listings-0.2.2").to_string_lossy(),
        ]
        .into_iter()
        .map(String::from)
        .collect::<Vec<_>>();
        assert_eq!(
            installs(runner),
            vec![expected_mdbook, expected_plugin],
            "each pin installs exactly once, however many builds use it"
        );
    }

    #[test]
    fn the_build_runs_the_pinned_mdbook_with_the_plugins_on_its_path() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();
        let pins = pins(Some("0.5.2"), &[versioned("mdbook-listings", "0.2.2")]);

        let html = toolchain
            .build(&pins, &book, &dir.path().join("out"))
            .unwrap();

        let build = runner.calls.borrow().last().cloned().unwrap();
        assert_eq!(
            build.program,
            root.join("mdbook-0.5.2/bin").join(exe("mdbook"))
        );
        assert_eq!(
            strs(&build.args),
            vec![
                "build".to_string(),
                book.to_string_lossy().into_owned(),
                "--dest-dir".to_string(),
                dir.path().join("out").to_string_lossy().into_owned(),
            ]
        );
        assert_eq!(
            build.path_prepend,
            vec![root.join("mdbook-listings-0.2.2/bin")],
            "mdbook finds the plugin by name, so its bin dir leads PATH"
        );
        assert_eq!(html, dir.path().join("out"));
    }

    #[test]
    fn a_plugins_assets_install_with_its_pinned_binary_in_the_book() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        let pins = pins(Some("0.5.2"), &[versioned("mdbook-listings", "0.2.2")]);

        toolchain
            .install_assets(&pins, "mdbook-listings", &["install".to_string()], &book)
            .unwrap();

        let call = runner.calls.borrow().last().cloned().unwrap();
        assert_eq!(
            call.program,
            root.join("mdbook-listings-0.2.2/bin")
                .join(exe("mdbook-listings"))
        );
        assert_eq!(strs(&call.args), vec!["install".to_string()]);
        assert_eq!(call.cwd.as_deref(), Some(book.as_path()));
        assert_eq!(
            installs(runner).len(),
            1,
            "only the plugin whose assets are wanted installs now; mdbook waits for the build"
        );
    }

    #[test]
    fn a_git_pin_installs_from_its_rev() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();
        let rev = "0123456789abcdef0123456789abcdef01234567";
        let pins = pins(
            Some("0.5.2"),
            &[from_git("mdbook-admonish", Some(rev), None)],
        );

        toolchain
            .build(&pins, &book, &dir.path().join("out"))
            .unwrap();

        let plugin_install = installs(runner).into_iter().nth(1).unwrap();
        assert_eq!(
            plugin_install,
            vec![
                "install".to_string(),
                "--git".to_string(),
                "https://example.com/repo".to_string(),
                "--rev".to_string(),
                rev.to_string(),
                "--locked".to_string(),
                "--root".to_string(),
                root.join(format!("mdbook-admonish-{rev}"))
                    .to_string_lossy()
                    .into_owned(),
                "mdbook-admonish".to_string(),
            ]
        );
        let build = runner.calls.borrow().last().cloned().unwrap();
        assert_eq!(
            build.path_prepend,
            vec![root.join(format!("mdbook-admonish-{rev}/bin"))]
        );
    }

    /// A branch tip moves, so the pin is the commit it resolves to today:
    /// a fresh tip installs again, the same tip is reused.
    #[test]
    fn a_branch_pin_is_resolved_to_its_tip_and_cached_by_it() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();
        let pins = pins(None, &[from_git("mdbook-admonish", None, Some("feat/x"))]);

        toolchain
            .build(&pins, &book, &dir.path().join("out"))
            .unwrap();

        let tip = "a".repeat(40);
        let calls = runner.calls.borrow();
        let ls_remote = calls
            .iter()
            .find(|c| c.program == Path::new("git"))
            .unwrap();
        assert_eq!(
            strs(&ls_remote.args),
            vec![
                "ls-remote".to_string(),
                "https://example.com/repo".to_string(),
                "refs/heads/feat/x".to_string()
            ]
        );
        assert_eq!(
            installs(runner),
            vec![vec![
                "install".to_string(),
                "--git".to_string(),
                "https://example.com/repo".to_string(),
                "--rev".to_string(),
                tip.clone(),
                "--locked".to_string(),
                "--root".to_string(),
                root.join(format!("mdbook-admonish-{tip}"))
                    .to_string_lossy()
                    .into_owned(),
                "mdbook-admonish".to_string(),
            ]],
            "the branch installs as the commit at its tip"
        );
        assert_eq!(
            calls.last().unwrap().program,
            Path::new("mdbook"),
            "no mdbook pin means PATH's mdbook"
        );
    }

    /// A rev that is not a commit id (`main`, a tag, a short prefix) moves
    /// like a branch, so it is looked up the same way and cached by the
    /// commit it names today. Only the unreleased edition gets this far.
    #[test]
    fn a_rev_that_is_not_a_commit_id_is_resolved_like_a_branch() {
        let (dir, toolchain, runner) = toolchain();
        let root = dir.path().join("toolchains");
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();
        let pins = pins(None, &[from_git("mdbook-admonish", Some("v1.2"), None)]);

        toolchain
            .build(&pins, &book, &dir.path().join("out"))
            .unwrap();

        let tip = "a".repeat(40);
        let calls = runner.calls.borrow();
        let ls_remote = calls
            .iter()
            .find(|c| c.program == Path::new("git"))
            .expect("the rev is looked up");
        assert_eq!(
            strs(&ls_remote.args),
            vec![
                "ls-remote".to_string(),
                "https://example.com/repo".to_string(),
                "v1.2".to_string()
            ]
        );
        assert_eq!(
            installs(runner)[0][7],
            root.join(format!("mdbook-admonish-{tip}"))
                .to_string_lossy(),
            "installed under the commit, not the name"
        );
    }

    #[test]
    fn a_commit_id_is_forty_hex_digits_and_nothing_else() {
        for (rev, is_id) in [
            ("0123456789abcdef0123456789abcdef01234567", true),
            ("0123456789ABCDEF0123456789ABCDEF01234567", true),
            ("0123456789abcdef0123456789abcdef0123456", false),
            ("0123456789abcdef0123456789abcdef012345678", false),
            ("g123456789abcdef0123456789abcdef01234567", false),
            ("abc", false),
            ("main", false),
        ] {
            assert_eq!(is_commit_id(rev), is_id, "rev {rev:?}");
        }
    }

    #[test]
    fn without_pins_the_tools_come_from_path() {
        let (dir, toolchain, runner) = toolchain();
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        fs::write(book.join("book.toml"), "[book]\n").unwrap();

        toolchain
            .install_assets(
                &ToolchainFile::default(),
                "mdbook-listings",
                &["install".to_string()],
                &book,
            )
            .unwrap();
        toolchain
            .build(&ToolchainFile::default(), &book, &dir.path().join("out"))
            .unwrap();

        assert!(installs(runner).is_empty(), "nothing to install");
        let programs: Vec<PathBuf> = runner
            .calls
            .borrow()
            .iter()
            .map(|c| c.program.clone())
            .collect();
        assert_eq!(
            programs,
            vec![PathBuf::from("mdbook-listings"), PathBuf::from("mdbook")]
        );
    }

    #[test]
    fn an_install_failure_names_the_tool_and_its_pin() {
        let (dir, toolchain, _runner) = toolchain();
        let book = dir.path().join("book");
        fs::create_dir_all(&book).unwrap();
        let pins = pins(None, &[versioned("broken", "1.0.0")]);

        let err = toolchain
            .install_assets(&pins, "broken", &["install".to_string()], &book)
            .unwrap_err();

        assert_eq!(
            err,
            ToolError::Install {
                tool: "broken".to_string(),
                pin: "1.0.0".to_string(),
                output: "error: failed to compile `broken`".to_string(),
            }
        );
    }

    #[test]
    fn a_branch_whose_tip_cannot_be_resolved_is_an_install_error() {
        struct NoRemote;
        impl Runner for NoRemote {
            fn run(&self, invocation: &Invocation) -> Result<String, String> {
                assert_eq!(invocation.program, Path::new("git"));
                Err("fatal: could not read from remote".to_string())
            }
        }
        let dir = tempfile::tempdir().unwrap();
        let toolchain = InstalledToolchain::with_runner(dir.path(), Box::new(NoRemote));
        let pins = pins(None, &[from_git("mdbook-admonish", None, Some("main"))]);

        let err = toolchain
            .build(&pins, dir.path(), &dir.path().join("out"))
            .unwrap_err();

        match err {
            ToolError::Install { tool, pin, output } => {
                assert_eq!(
                    (tool.as_str(), pin.as_str()),
                    ("mdbook-admonish", "https://example.com/repo#main")
                );
                assert!(
                    output.contains("could not read from remote"),
                    "got {output}"
                );
            }
            other => panic!("expected Install, got {other:?}"),
        }
    }

    #[test]
    fn the_html_root_is_a_subdirectory_only_with_several_backends() {
        let dest = Path::new("out");
        for (book_toml, root) in [
            ("[book]\ntitle = \"T\"\n", "out"),
            ("[output]\n", "out"),
            ("[output.html]\n", "out"),
            ("[output.markdown]\n", "out"),
            ("[output.html]\n[output.markdown]\n", "out/html"),
            (
                "[output.html]\n[output.markdown]\n[output.linkcheck]\n",
                "out/html",
            ),
        ] {
            assert_eq!(
                html_root(book_toml, dest),
                Path::new(root),
                "book.toml:\n{book_toml}"
            );
        }
    }

    #[test]
    fn the_process_runner_reports_a_failing_tools_output() {
        let dir = tempfile::tempdir().unwrap();
        let git = |args: &[&str]| Invocation {
            program: PathBuf::from("git"),
            args: args.iter().map(OsString::from).collect(),
            cwd: Some(dir.path().to_path_buf()),
            path_prepend: Vec::new(),
        };

        let version = ProcessRunner
            .run(&git(&["--version"]))
            .expect("a tool that succeeds is Ok");
        let failed = ProcessRunner
            .run(&git(&["no-such-subcommand"]))
            .unwrap_err();
        let missing = ProcessRunner
            .run(&Invocation {
                program: PathBuf::from("panschema-no-such-tool"),
                args: Vec::new(),
                cwd: None,
                path_prepend: Vec::new(),
            })
            .unwrap_err();

        assert!(
            version.starts_with("git version"),
            "stdout comes back; got {version}"
        );
        assert!(failed.contains("no-such-subcommand"), "got {failed}");
        assert!(missing.contains("panschema-no-such-tool"), "got {missing}");
    }

    /// The prepended directories lead `PATH` for the process, ahead of
    /// whatever the parent had.
    #[cfg(unix)]
    #[test]
    fn the_process_runner_prepends_to_the_path() {
        let dir = tempfile::tempdir().unwrap();
        let out = ProcessRunner
            .run(&Invocation {
                program: PathBuf::from("/bin/sh"),
                args: vec!["-c".into(), "echo \"$PATH\"".into()],
                cwd: None,
                path_prepend: vec![dir.path().join("a"), dir.path().join("b")],
            })
            .unwrap();

        assert!(
            out.starts_with(&format!(
                "{}:{}:",
                dir.path().join("a").display(),
                dir.path().join("b").display()
            )),
            "got {out}"
        );
    }

    #[test]
    #[ignore = "needs mdbook 0.5 on PATH; run with cargo nextest run --run-ignored only -E 'test(real_mdbook)'"]
    fn real_mdbook_builds_into_the_destination() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        fs::create_dir_all(book.join("src")).unwrap();
        fs::write(book.join("book.toml"), "[book]\ntitle = \"T\"\n").unwrap();
        fs::write(
            book.join("src/SUMMARY.md"),
            "# Summary\n\n- [Chapter](chapter.md)\n",
        )
        .unwrap();
        fs::write(book.join("src/chapter.md"), "# Chapter\n").unwrap();

        let root = InstalledToolchain::new(dir.path().join("toolchains"))
            .build(&ToolchainFile::default(), &book, &dir.path().join("out"))
            .unwrap();

        assert_eq!(root, dir.path().join("out"));
        assert!(root.join("chapter.html").is_file());
    }

    #[test]
    #[ignore = "needs mdbook 0.5 on PATH; run with cargo nextest run --run-ignored only -E 'test(real_mdbook)'"]
    fn real_mdbook_with_two_backends_reports_the_html_root() {
        let dir = tempfile::tempdir().unwrap();
        let book = dir.path().join("book");
        fs::create_dir_all(book.join("src")).unwrap();
        fs::write(
            book.join("book.toml"),
            "[book]\ntitle = \"T\"\n[output.html]\n[output.markdown]\n",
        )
        .unwrap();
        fs::write(
            book.join("src/SUMMARY.md"),
            "# Summary\n\n- [Chapter](chapter.md)\n",
        )
        .unwrap();
        fs::write(book.join("src/chapter.md"), "# Chapter\n").unwrap();

        let root = InstalledToolchain::new(dir.path().join("toolchains"))
            .build(&ToolchainFile::default(), &book, &dir.path().join("out"))
            .unwrap();

        assert_eq!(root, dir.path().join("out/html"));
        assert!(root.join("chapter.html").is_file());
    }
}
