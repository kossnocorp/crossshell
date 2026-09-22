use crate::{
    Flow, Outcome,
    eval::Evaluator,
    io::{self, Io},
};
use anyhow::{Result, bail};

impl Evaluator<'_, '_, '_> {
    pub fn builtin(&mut self, args: &[String], io: &Io, tested: bool) -> Result<Option<Outcome>> {
        let name = args[0].as_str();
        let args = &args[1..];
        let mut result = Outcome::status(0);
        match name {
            ":" | "true" => {}
            "false" => result.status = 1,
            "cd" => {
                let args = if args.first().is_some_and(|s| s == "--") {
                    &args[1..]
                } else {
                    args
                };
                if args.len() > 1 {
                    io.write(2, "cd: too many arguments\n")?;
                    result.status = 1;
                } else {
                    let target = match args.first().map(String::as_str) {
                        None => self
                            .value("HOME")?
                            .ok_or_else(|| anyhow::anyhow!("cd: HOME not set"))?,
                        Some("-") => self
                            .value("OLDPWD")?
                            .ok_or_else(|| anyhow::anyhow!("cd: OLDPWD not set"))?,
                        Some(s) => s.into(),
                    };
                    match self.path(&target).canonicalize() {
                        Ok(path) if path.is_dir() => {
                            let previous = self.cwd.clone();
                            self.set("OLDPWD", previous.into_os_string());
                            self.cwd = path.clone();
                            self.set("PWD", path.into_os_string());
                            self.exported.insert("PWD".into());
                            self.exported.insert("OLDPWD".into());
                            if args.first().is_some_and(|s| s == "-") {
                                io.write(1, format!("{}\n", self.cwd.display()))?;
                            }
                        }
                        Ok(_) => {
                            io.write(2, format!("cd: {target}: not a directory\n"))?;
                            result.status = 1;
                        }
                        Err(error) => {
                            io.write(2, format!("cd: {target}: {error}\n"))?;
                            result.status = 1;
                        }
                    }
                }
            }
            "exit" | "return" => {
                if args.len() > 1 {
                    io.write(2, format!("{name}: too many arguments\n"))?;
                    result.status = 1;
                } else if name == "return" && self.function_depth == 0 {
                    io.write(2, "return: not in a function\n")?;
                    result.status = 1;
                } else {
                    result.status = match args.first() {
                        Some(s) => s.parse::<i64>().map(|n| n as u8).unwrap_or(2),
                        None => self.last_status,
                    };
                    result.flow = if name == "exit" {
                        Flow::Exit
                    } else {
                        Flow::Return
                    };
                }
            }
            "break" | "continue" => {
                let count = args
                    .first()
                    .map(|s| s.parse::<usize>())
                    .transpose()?
                    .unwrap_or(1);
                if args.len() > 1 || count == 0 || self.loop_depth == 0 {
                    io.write(2, format!("{name}: invalid loop control\n"))?;
                    result.status = 1;
                } else {
                    let count = count.min(self.loop_depth);
                    result.flow = if name == "break" {
                        Flow::Break(count)
                    } else {
                        Flow::Continue(count)
                    };
                }
            }
            "export" | "local" => {
                if name == "local" && self.function_depth == 0 {
                    bail!("local: not in a function");
                }
                let mut unexport = false;
                for arg in args {
                    if arg == "-n" && name == "export" {
                        unexport = true;
                        continue;
                    }
                    if arg == "--" {
                        continue;
                    }
                    let (key, value) = arg
                        .split_once('=')
                        .map(|(k, v)| (k, Some(v)))
                        .unwrap_or((arg, None));
                    if !valid_name(key) {
                        io.write(2, format!("{name}: {key}: invalid variable name\n"))?;
                        result.status = 1;
                        continue;
                    }
                    if name == "local" {
                        let previous = self.variables.get(key).cloned();
                        self.locals
                            .last_mut()
                            .unwrap()
                            .entry(key.into())
                            .or_insert(previous);
                    }
                    if let Some(value) = value {
                        self.set(key, value);
                    }
                    if name == "export" {
                        if unexport {
                            self.exported.remove(key);
                        } else {
                            self.exported.insert(key.into());
                        }
                    }
                }
                if args.is_empty() && name == "export" {
                    for (key, value) in &self.variables {
                        if self.exported.contains(key) {
                            io.write(
                                1,
                                format!(
                                    "export {key}='{}'\n",
                                    value.to_string_lossy().replace('\'', "'\\''")
                                ),
                            )?;
                        }
                    }
                }
            }
            "unset" => {
                let mut functions = false;
                for arg in args {
                    if arg == "-f" {
                        functions = true;
                        continue;
                    }
                    if matches!(arg.as_str(), "-v" | "--") {
                        continue;
                    }
                    if functions {
                        self.functions.remove(arg.as_str());
                    } else {
                        self.variables.remove(arg);
                        self.exported.remove(arg);
                    }
                }
            }
            "shift" => {
                let count = args
                    .first()
                    .map(|s| s.parse::<usize>())
                    .transpose()?
                    .unwrap_or(1);
                if args.len() > 1 || count > self.args.len() {
                    result.status = 1;
                } else {
                    self.args.drain(..count);
                }
            }
            "set" => {
                let mut i = 0;
                while i < args.len() {
                    let arg = &args[i];
                    if arg == "--" {
                        self.args = args[i + 1..].to_vec();
                        break;
                    }
                    if !arg.starts_with(['-', '+']) {
                        self.args = args[i..].to_vec();
                        break;
                    }
                    let enabled = arg.starts_with('-');
                    if matches!(arg.as_str(), "-o" | "+o") {
                        i += 1;
                        let option = args
                            .get(i)
                            .ok_or_else(|| anyhow::anyhow!("set: missing option name"))?;
                        match option.as_str() {
                            "errexit" => self.errexit = enabled,
                            "nounset" => self.nounset = enabled,
                            "noglob" => self.noglob = enabled,
                            "pipefail" => self.pipefail = enabled,
                            _ => bail!("set: unsupported option {option}"),
                        }
                    } else {
                        for c in arg[1..].chars() {
                            match c {
                                'e' => self.errexit = enabled,
                                'u' => self.nounset = enabled,
                                'f' => self.noglob = enabled,
                                _ => bail!("set: unsupported option {c}"),
                            }
                        }
                    }
                    i += 1;
                }
                if args.is_empty() {
                    for (key, value) in &self.variables {
                        io.write(1, format!("{key}={}\n", value.to_string_lossy()))?;
                    }
                }
            }
            "read" => {
                let mut names = args;
                let raw = names.first().is_some_and(|s| s == "-r");
                if raw {
                    names = &names[1..];
                }
                let Some(mut line) = io.read_line()? else {
                    return Ok(Some(Outcome::status(1)));
                };
                if !raw {
                    while line.ends_with('\\') {
                        line.pop();
                        match io.read_line()? {
                            Some(next) => line.push_str(&next),
                            None => break,
                        }
                    }
                    let mut decoded = String::new();
                    let mut chars = line.chars();
                    while let Some(c) = chars.next() {
                        if c == '\\' {
                            if let Some(next) = chars.next() {
                                decoded.push(next);
                            }
                        } else {
                            decoded.push(c);
                        }
                    }
                    line = decoded;
                }
                if names.is_empty() {
                    self.set("REPLY", line);
                } else {
                    let ifs = self.value("IFS")?.unwrap_or_else(|| " \t\n".into());
                    let mut remaining =
                        line.trim_matches(|c| ifs.contains(c) && " \t\n".contains(c));
                    for (i, name) in names.iter().enumerate() {
                        if !valid_name(name) {
                            bail!("read: invalid variable name {name}");
                        }
                        if i + 1 == names.len() {
                            self.set(name, remaining);
                            break;
                        }
                        if let Some(index) = remaining.find(|c| ifs.contains(c)) {
                            self.set(name, &remaining[..index]);
                            let c = remaining[index..].chars().next().unwrap();
                            remaining = remaining[index + c.len_utf8()..]
                                .trim_start_matches(|c| ifs.contains(c) && " \t\n".contains(c));
                        } else {
                            self.set(name, remaining);
                            remaining = "";
                        }
                    }
                }
            }
            "wait" => {
                if args.is_empty() {
                    self.wait_all()?;
                } else {
                    for arg in args {
                        let id = arg.trim_start_matches('%').parse::<usize>()?;
                        result.status = if let Some(handle) = self.jobs.remove(&id) {
                            io::join(handle)?.status
                        } else {
                            127
                        };
                    }
                }
            }
            "command" => {
                if args.first().is_some_and(|s| s.starts_with('-')) {
                    bail!("command: options are not implemented");
                }
                if !args.is_empty() {
                    result = self.invoke(args, io, tested, true)?;
                }
            }
            "builtin" => {
                if !args.is_empty() {
                    result = self
                        .builtin(args, io, tested)?
                        .unwrap_or(Outcome::status(1));
                }
            }
            // These alter shell state or evaluate shell syntax. Never accidentally
            // pass them to a host executable and imply they worked in this shell.
            "eval" | "source" | "." | "exec" | "trap" | "alias" | "unalias" | "declare"
            | "typeset" | "readonly" | "umask" | "getopts" | "shopt" | "jobs" | "fg" | "bg" => {
                bail!("Unsupported shell builtin: {name}")
            }
            _ => return Ok(None),
        }
        Ok(Some(result))
    }
}

pub(crate) fn valid_name(name: &str) -> bool {
    let mut chars = name.chars();
    chars
        .next()
        .is_some_and(|c| c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}
