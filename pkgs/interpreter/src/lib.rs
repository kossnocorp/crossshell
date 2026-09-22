//! Interpretation of Crossshell's own parser and AST, with native uutils processes.
//! Call [`dispatch_utility`] at the start of the embedding executable's `main`.

mod arithmetic;
mod builtins;
mod eval;
mod expand;
mod io;
mod utilities;
pub use utilities::{UTILITIES, dispatch_utility};

use anyhow::{Context, Result};
use crossshell::{CshAst, CshParser};
use io::Io;
use std::{
    collections::{BTreeMap, HashSet},
    ffi::OsString,
    path::{Path, PathBuf},
};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flow {
    Normal,
    Exit,
    Return,
    Break(usize),
    Continue(usize),
}

#[derive(Clone, Copy)]
struct Outcome {
    status: u8,
    flow: Flow,
}
impl Outcome {
    fn status(status: u8) -> Self {
        Self {
            status,
            flow: Flow::Normal,
        }
    }
}

/// A persistent shell environment. Variables and working directories are private
/// to this instance. Function definitions belong to each borrowed AST execution.
pub struct CshInterpreter {
    state: State,
    _utilities: tempfile::TempDir,
}

#[derive(Clone)]
struct State {
    variables: BTreeMap<String, OsString>,
    exported: HashSet<String>,
    other_environment: Vec<(OsString, OsString)>,
    cwd: PathBuf,
    name: String,
    args: Vec<String>,
    last_status: u8,
    last_job: Option<usize>,
    next_job: usize,
    loop_depth: usize,
    function_depth: usize,
    locals: Vec<BTreeMap<String, Option<OsString>>>,
    errexit: bool,
    nounset: bool,
    noglob: bool,
    pipefail: bool,
    substitution_status: Option<u8>,
}

impl CshInterpreter {
    /// Create a shell using an executable that calls [`dispatch_utility`].
    /// Bundled utilities are placed before inherited PATH entries.
    pub fn new(executable: impl AsRef<Path>) -> Result<Self> {
        let executable = executable
            .as_ref()
            .canonicalize()
            .context("Failed to resolve utility host")?;
        let utilities = tempfile::Builder::new().prefix("crossshell-").tempdir()?;
        for name in UTILITIES {
            #[cfg(unix)]
            std::os::unix::fs::symlink(&executable, utilities.path().join(name))?;
            #[cfg(windows)]
            {
                let target = utilities.path().join(format!("{name}.exe"));
                if std::fs::hard_link(&executable, &target).is_err() {
                    std::fs::copy(&executable, &target)?;
                }
            }
        }
        let mut variables = BTreeMap::new();
        let mut other_environment = Vec::new();
        for (key, value) in std::env::vars_os() {
            match key.into_string() {
                Ok(key) => {
                    variables.insert(key, value);
                }
                Err(key) => other_environment.push((key, value)),
            }
        }
        let mut paths = vec![utilities.path().to_path_buf()];
        if let Some(path) = variables.get("PATH") {
            paths.extend(std::env::split_paths(path));
        }
        variables.insert("PATH".into(), std::env::join_paths(paths)?);
        let cwd = std::env::current_dir()?;
        variables.insert("PWD".into(), cwd.as_os_str().into());
        let exported = variables.keys().cloned().collect();
        Ok(Self {
            state: State {
                variables,
                exported,
                other_environment,
                cwd,
                name: "cssh".into(),
                args: Vec::new(),
                last_status: 0,
                last_job: None,
                next_job: 1,
                loop_depth: 0,
                function_depth: 0,
                locals: Vec::new(),
                errexit: false,
                nounset: false,
                noglob: false,
                pipefail: false,
                substitution_status: None,
            },
            _utilities: utilities,
        })
    }

    /// Evaluate the supplied AST. Its source text is never parsed or executed.
    /// Functions and jobs borrow it directly. All jobs are joined before this
    /// method returns, so neither the AST nor its source needs to be cloned.
    pub fn run_ast(&mut self, ast: &CshAst<'_>) -> Result<u8> {
        let io = Io::inherited()?;
        std::thread::scope(|scope| {
            let mut evaluator = eval::Evaluator::new(&mut self.state, ast, scope);
            let result = evaluator.list(&ast.commands, &io, false);
            let jobs = evaluator.wait_all();
            let result = result?;
            jobs?;
            Ok(result.status)
        })
    }

    /// Parse source with `CshParser` and evaluate the resulting AST.
    pub fn run(&mut self, source: &str, name: &str) -> Result<u8> {
        self.state.name = name.into();
        let ast = CshParser::parse(source).map_err(|e| anyhow::anyhow!("{name}: {e}"))?;
        self.run_ast(&ast)
    }

    /// Execute a script with `$0` and positional arguments, parsing it once.
    pub fn run_script(&mut self, path: impl AsRef<Path>, args: &[String]) -> Result<u8> {
        let path = path.as_ref();
        let source = std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read {}", path.display()))?;
        self.state.args = args.to_vec();
        self.run(&source, &path.to_string_lossy())
    }

    /// Set `$0` and positional parameters before evaluating a caller-owned AST.
    pub fn set_args(&mut self, name: impl Into<String>, args: Vec<String>) {
        self.state.name = name.into();
        self.state.args = args;
    }
}

impl State {
    fn fork(&self) -> Self {
        Self {
            loop_depth: 0,
            next_job: 1,
            substitution_status: None,
            ..self.clone()
        }
    }

    fn value(&self, name: &str) -> Result<Option<String>> {
        Ok(match name {
            "?" => Some(self.last_status.to_string()),
            "$" => Some(std::process::id().to_string()),
            "!" => self.last_job.map(|n| n.to_string()),
            "#" => Some(self.args.len().to_string()),
            "0" => Some(self.name.clone()),
            "@" | "*" => Some(self.args.join(&self.ifs_separator()?)),
            "-" => Some(format!(
                "{}{}{}",
                if self.errexit { "e" } else { "" },
                if self.nounset { "u" } else { "" },
                if self.noglob { "f" } else { "" }
            )),
            n if n.parse::<usize>().is_ok() => n
                .parse::<usize>()
                .ok()
                .and_then(|n| n.checked_sub(1))
                .and_then(|i| self.args.get(i).cloned()),
            n => self
                .variables
                .get(n)
                .map(|s| {
                    s.clone()
                        .into_string()
                        .map_err(|_| anyhow::anyhow!("{n}: value is not valid Unicode"))
                })
                .transpose()?,
        })
    }

    fn ifs_separator(&self) -> Result<String> {
        Ok(self
            .value("IFS")?
            .unwrap_or_else(|| " \t\n".into())
            .chars()
            .take(1)
            .collect())
    }

    fn set(&mut self, name: &str, value: impl Into<OsString>) {
        self.variables.insert(name.into(), value.into());
    }
    fn path(&self, path: impl AsRef<Path>) -> PathBuf {
        self.cwd.join(path)
    }
}
