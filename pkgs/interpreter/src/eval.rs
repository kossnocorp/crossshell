use crate::{
    Flow, Outcome, State,
    io::{self, Io},
};
use anyhow::{Context, Result, bail};
use crossshell::*;
use std::{
    collections::BTreeMap,
    fs::OpenOptions,
    ops::{Deref, DerefMut},
    process::Command,
    thread::{Scope, ScopedJoinHandle},
};

pub(crate) struct Evaluator<'state, 'scope, 'ast> {
    pub state: &'state mut State,
    pub ast: &'ast CshAst<'ast>,
    pub scope: &'scope Scope<'scope, 'ast>,
    pub functions: BTreeMap<&'ast str, CshAstNodeId>,
    pub jobs: BTreeMap<usize, ScopedJoinHandle<'scope, Result<Outcome>>>,
}

impl Deref for Evaluator<'_, '_, '_> {
    type Target = State;
    fn deref(&self) -> &State {
        self.state
    }
}
impl DerefMut for Evaluator<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut State {
        self.state
    }
}

impl<'state, 'scope, 'ast> Evaluator<'state, 'scope, 'ast> {
    pub fn new(
        state: &'state mut State,
        ast: &'ast CshAst<'ast>,
        scope: &'scope Scope<'scope, 'ast>,
    ) -> Self {
        Self {
            state,
            ast,
            scope,
            functions: BTreeMap::new(),
            jobs: BTreeMap::new(),
        }
    }

    pub fn list(&mut self, list: &[CshAstNodeId], io: &Io, tested: bool) -> Result<Outcome> {
        let mut result = Outcome::status(0);
        for id in list {
            result = self.eval(*id, io, tested)?;
            if result.flow != Flow::Normal {
                break;
            }
        }
        Ok(result)
    }

    pub fn eval(&mut self, id: CshAstNodeId, io: &Io, tested: bool) -> Result<Outcome> {
        let result = self.eval_inner(id, io, tested);
        let mut result = result.with_context(|| {
            let span = self.ast.spans.get(id.0).cloned().unwrap_or_default();
            format!("{}:{}..{}", self.name, span.start, span.end)
        })?;
        self.last_status = result.status;
        let checks_errexit = matches!(
            &self.ast[id],
            CshAstExpression::Command(_)
                | CshAstExpression::Test(_)
                | CshAstExpression::Arithmetic(_)
                | CshAstExpression::Subshell(_)
                | CshAstExpression::Redirected(_)
                | CshAstExpression::Binary(CshAstBinary {
                    operator: CshAstOperator::Pipe | CshAstOperator::PipeWithStderr,
                    ..
                })
        );
        if self.errexit
            && checks_errexit
            && !tested
            && result.status != 0
            && result.flow == Flow::Normal
        {
            result.flow = Flow::Exit;
        }
        Ok(result)
    }

    fn eval_inner(&mut self, id: CshAstNodeId, io: &Io, tested: bool) -> Result<Outcome> {
        let ast = self.ast;
        let node = ast
            .nodes
            .get(id.0)
            .ok_or_else(|| anyhow::anyhow!("Invalid AST node {}", id.0))?;
        use CshAstExpression as E;
        Ok(match node {
            E::Command(c) => self.command(c, io, tested)?,
            E::Function(f) => {
                self.functions.insert(f.name, f.body);
                Outcome::status(0)
            }
            E::Group(g) => self.list(&g.body, io, tested)?,
            E::Subshell(s) => {
                let mut state = self.state.fork();
                let mut child =
                    Self::new_child(&mut state, self.ast, self.scope, self.functions.clone());
                let result = child.list(&s.body, io, tested)?;
                child.wait_all()?;
                Outcome::status(result.status)
            }
            E::Negated(n) => {
                let result = self.eval(n.expression, io, true)?;
                Outcome::status(u8::from(result.status == 0))
            }
            E::Binary(b) => match b.operator {
                CshAstOperator::And | CshAstOperator::Or => {
                    let left = self.eval(b.left, io, true)?;
                    if left.flow != Flow::Normal {
                        left
                    } else if (left.status == 0) == (b.operator == CshAstOperator::And) {
                        self.eval(b.right, io, tested)?
                    } else {
                        left
                    }
                }
                CshAstOperator::Pipe | CshAstOperator::PipeWithStderr => self.pipeline(id, io)?,
            },
            E::Background(b) => {
                let mut child_io = io.clone();
                child_io.0.insert(0, io::input("")?);
                let job = self.next_job;
                self.next_job += 1;
                let handle = self.spawn(b.expression, child_io, false);
                self.jobs.insert(job, handle);
                self.last_job = Some(job);
                Outcome::status(0)
            }
            E::If(i) => {
                let mut result = Outcome::status(0);
                let mut matched = false;
                for branch in &i.branches {
                    let condition = self.list(&branch.condition, io, true)?;
                    if condition.flow != Flow::Normal {
                        result = condition;
                        matched = true;
                        break;
                    }
                    if condition.status == 0 {
                        result = self.list(&branch.body, io, tested)?;
                        matched = true;
                        break;
                    }
                }
                if !matched && let Some(otherwise) = &i.otherwise {
                    result = self.list(otherwise, io, tested)?;
                }
                result
            }
            E::For(f) => {
                let words = match &f.words {
                    Some(words) => self.words(words, io)?,
                    None => self.args.clone(),
                };
                self.loop_depth += 1;
                let result: Result<Outcome> = (|| {
                    let mut result = Outcome::status(0);
                    for word in words {
                        self.set(&f.variable, word);
                        result = self.list(&f.body, io, tested)?;
                        if loop_control(&mut result) {
                            break;
                        }
                    }
                    Ok(result)
                })();
                self.loop_depth -= 1;
                result?
            }
            E::Loop(l) => {
                self.loop_depth += 1;
                let result: Result<Outcome> = (|| {
                    let mut result = Outcome::status(0);
                    loop {
                        let condition = self.list(&l.condition, io, true)?;
                        if condition.flow != Flow::Normal {
                            result = condition;
                            break;
                        }
                        if (condition.status == 0) == l.until {
                            break;
                        }
                        result = self.list(&l.body, io, tested)?;
                        if loop_control(&mut result) {
                            break;
                        }
                    }
                    Ok(result)
                })();
                self.loop_depth -= 1;
                result?
            }
            E::Arithmetic(a) => Outcome::status(u8::from(self.arithmetic(a, io)? == 0)),
            E::ArithmeticFor(f) => {
                if let Some(init) = &f.clauses.init {
                    self.arithmetic(init, io)?;
                }
                self.loop_depth += 1;
                let result: Result<Outcome> = (|| {
                    let mut result = Outcome::status(0);
                    loop {
                        if let Some(condition) = &f.clauses.condition
                            && self.arithmetic(condition, io)? == 0
                        {
                            break;
                        }
                        result = self.list(&f.body, io, tested)?;
                        if loop_control(&mut result) {
                            break;
                        }
                        if let Some(update) = &f.clauses.update {
                            self.arithmetic(update, io)?;
                        }
                    }
                    Ok(result)
                })();
                self.loop_depth -= 1;
                result?
            }
            E::Test(c) => Outcome::status(u8::from(!self.condition(c, io)?)),
            E::Case(c) => {
                let value = self.scalar(&c.word, io)?;
                let mut fallthrough = false;
                let mut result = Outcome::status(0);
                for arm in &c.arms {
                    let mut matched = fallthrough;
                    if !matched {
                        for pattern in &arm.patterns {
                            let pattern = self.pattern(pattern, io)?;
                            if crate::expand::matches(&pattern, &value)? {
                                matched = true;
                                break;
                            }
                        }
                    }
                    if !matched {
                        continue;
                    }
                    result = self.list(&arm.body, io, tested)?;
                    if result.flow != Flow::Normal || arm.terminator == ";;" {
                        break;
                    }
                    fallthrough = arm.terminator == ";&";
                }
                result
            }
            E::Redirected(r) => {
                let redirected = self.redirects(&r.redirects, io);
                match redirected {
                    Ok(io) => self.eval(r.expression, &io, tested)?,
                    Err(error) => {
                        io.write(2, format!("cssh: {error:#}\n"))?;
                        Outcome::status(1)
                    }
                }
            }
        })
    }

    fn new_child<'s>(
        state: &'s mut State,
        ast: &'ast CshAst<'ast>,
        scope: &'scope Scope<'scope, 'ast>,
        functions: BTreeMap<&'ast str, CshAstNodeId>,
    ) -> Evaluator<'s, 'scope, 'ast> {
        Evaluator {
            state,
            ast,
            scope,
            functions,
            jobs: BTreeMap::new(),
        }
    }

    pub fn spawn(
        &self,
        node: CshAstNodeId,
        io: Io,
        tested: bool,
    ) -> ScopedJoinHandle<'scope, Result<Outcome>> {
        self.spawn_stage(node, io, tested, false)
    }

    fn spawn_stage(
        &self,
        node: CshAstNodeId,
        mut io: Io,
        tested: bool,
        pipe_stderr: bool,
    ) -> ScopedJoinHandle<'scope, Result<Outcome>> {
        let mut state = self.state.fork();
        let (ast, scope, functions) = (self.ast, self.scope, self.functions.clone());
        scope.spawn(move || {
            let mut child = Evaluator::new_child(&mut state, ast, scope, functions);
            // |& applies its implicit 2>&1 after the stage's explicit redirects.
            let result = if pipe_stderr {
                if let CshAstExpression::Redirected(r) = &ast[node] {
                    match child.redirects(&r.redirects, &io) {
                        Ok(mut redirected) => {
                            redirected.0.insert(2, redirected.get(1)?);
                            child.eval(r.expression, &redirected, tested)
                        }
                        Err(error) => {
                            io.write(2, format!("cssh: {error:#}\n"))?;
                            Ok(Outcome::status(1))
                        }
                    }
                } else {
                    io.0.insert(2, io.get(1)?);
                    child.eval(node, &io, tested)
                }
            } else {
                child.eval(node, &io, tested)
            };
            let jobs = child.wait_all();
            jobs?;
            result
        })
    }

    fn pipeline(&mut self, id: CshAstNodeId, io: &Io) -> Result<Outcome> {
        fn flatten(ast: &CshAst<'_>, id: CshAstNodeId, nodes: &mut Vec<(CshAstNodeId, bool)>) {
            if let CshAstExpression::Binary(b) = &ast[id]
                && matches!(
                    b.operator,
                    CshAstOperator::Pipe | CshAstOperator::PipeWithStderr
                )
            {
                flatten(ast, b.left, nodes);
                if let Some(last) = nodes.last_mut() {
                    last.1 = b.operator == CshAstOperator::PipeWithStderr;
                }
                flatten(ast, b.right, nodes);
            } else {
                nodes.push((id, false));
            }
        }
        let mut nodes = Vec::new();
        flatten(self.ast, id, &mut nodes);
        let mut handles = Vec::new();
        let mut input = io.get(0).ok();
        for (index, (node, stderr)) in nodes.iter().enumerate() {
            let mut stage = io.clone();
            match input.take() {
                Some(input) => {
                    stage.0.insert(0, input);
                }
                None => {
                    stage.0.remove(&0);
                }
            }
            if index + 1 < nodes.len() {
                let (reader, writer) = io::pipe()?;
                stage.0.insert(1, writer);
                input = Some(reader);
            }
            handles.push(self.spawn_stage(*node, stage, true, *stderr));
        }
        let mut status = 0;
        let mut first_error = None;
        for handle in handles {
            match io::join(handle) {
                Ok(result) => {
                    if !self.pipefail || result.status != 0 {
                        status = result.status;
                    }
                }
                Err(error) => {
                    if first_error.is_none() {
                        first_error = Some(error);
                    }
                }
            }
        }
        if let Some(error) = first_error {
            return Err(error);
        }
        Ok(Outcome::status(status))
    }

    pub fn wait_all(&mut self) -> Result<u8> {
        let mut status = 0;
        let mut error = None;
        for (_, handle) in std::mem::take(&mut self.jobs) {
            match io::join(handle) {
                Ok(result) => status = result.status,
                Err(e) => {
                    if error.is_none() {
                        error = Some(e);
                    }
                }
            }
        }
        if let Some(error) = error {
            return Err(error);
        }
        Ok(status)
    }

    fn command(&mut self, c: &CshAstCommand<'_>, io: &Io, tested: bool) -> Result<Outcome> {
        self.substitution_status = None;
        let mut args = Vec::new();
        if let Some(name) = &c.name {
            args.extend(self.words(std::slice::from_ref(name), io)?);
        }
        args.extend(self.words(&c.args, io)?);
        let mut saved = Vec::new();
        for assignment in &c.assignments {
            let value = self.assignment(assignment, io)?;
            saved.push((
                assignment.name.to_owned(),
                self.variables.get(assignment.name).cloned(),
                self.exported.contains(assignment.name),
            ));
            self.set(assignment.name, value);
            if !args.is_empty() {
                self.exported.insert(assignment.name.into());
            }
        }
        if args.is_empty() {
            return Ok(Outcome::status(self.substitution_status.unwrap_or(0)));
        }
        let result = self.invoke(&args, io, tested, false);
        for (name, value, exported) in saved.into_iter().rev() {
            match value {
                Some(value) => {
                    self.variables.insert(name.clone(), value);
                }
                None => {
                    self.variables.remove(&name);
                }
            }
            if exported {
                self.exported.insert(name);
            } else {
                self.exported.remove(&name);
            }
        }
        result
    }

    pub fn invoke(
        &mut self,
        args: &[String],
        io: &Io,
        tested: bool,
        skip_functions: bool,
    ) -> Result<Outcome> {
        if !skip_functions && let Some(body) = self.functions.get(args[0].as_str()).copied() {
            if self.function_depth >= 256 {
                bail!("Function recursion exceeds 256 calls");
            }
            let previous = std::mem::replace(&mut self.args, args[1..].to_vec());
            self.function_depth += 1;
            self.locals.push(BTreeMap::new());
            let result = self.eval(body, io, tested);
            for (name, value) in self.locals.pop().unwrap() {
                match value {
                    Some(value) => {
                        self.variables.insert(name, value);
                    }
                    None => {
                        self.variables.remove(&name);
                    }
                }
            }
            self.args = previous;
            self.function_depth -= 1;
            let mut result = result?;
            if result.flow == Flow::Return {
                result.flow = Flow::Normal;
            }
            return Ok(result);
        }
        if let Some(result) = self.builtin(args, io, tested)? {
            return Ok(result);
        }
        self.external(args, io).map(Outcome::status)
    }

    pub fn external(&self, args: &[String], io: &Io) -> Result<u8> {
        let name = &args[0];
        let mut denied = false;
        let candidates =
            if std::path::Path::new(name).components().count() > 1 || name.contains('/') {
                vec![self.path(name)]
            } else {
                let mut candidates = Vec::new();
                if let Some(path) = self.variables.get("PATH") {
                    for directory in std::env::split_paths(path) {
                        let candidate = self.path(directory).join(name);
                        #[cfg(windows)]
                        if candidate.extension().is_none() {
                            candidates.push(candidate.with_extension("exe"));
                        }
                        candidates.push(candidate);
                    }
                }
                candidates
            };
        for candidate in candidates {
            let mut command = Command::new(&candidate);
            command
                .args(&args[1..])
                .current_dir(&self.cwd)
                .env_clear()
                .envs(self.other_environment.iter().cloned());
            for key in &self.exported {
                if let Some(value) = self.variables.get(key) {
                    command.env(key, value);
                }
            }
            io.configure(&mut command)?;
            let child = command.spawn();
            // Drop parent's extra pipe handles immediately, before waiting.
            drop(command);
            match child {
                Ok(mut child) => return Ok(io::status(child.wait()?)),
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
                Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
                    denied = true;
                    continue;
                }
                Err(error) => {
                    io.write(2, format!("{name}: {error}\n"))?;
                    return Ok(126);
                }
            }
        }
        io.write(
            2,
            format!(
                "{name}: {}\n",
                if denied {
                    "permission denied"
                } else {
                    "command not found"
                }
            ),
        )?;
        Ok(if denied { 126 } else { 127 })
    }

    fn redirects(&mut self, redirects: &[CshAstRedirect<'_>], original: &Io) -> Result<Io> {
        use CshAstRedirectOperator as R;
        let mut io = original.clone();
        for redirect in redirects {
            let default = if matches!(
                redirect.operator,
                R::Input
                    | R::ReadWrite
                    | R::DuplicateInput
                    | R::CloseInput
                    | R::HereDocument
                    | R::HereDocumentStripTabs
                    | R::HereString
            ) {
                0
            } else {
                1
            };
            let fd = match &redirect.descriptor {
                CshAstDescriptor::Default => default,
                CshAstDescriptor::Number(n) => n.parse()?,
                CshAstDescriptor::Variable(name) => {
                    let fd = (10..1024)
                        .find(|fd| !io.0.contains_key(fd))
                        .ok_or_else(|| anyhow::anyhow!("No free file descriptor"))?;
                    self.set(name, fd.to_string());
                    fd
                }
            };
            if matches!(
                redirect.operator,
                R::HereDocument | R::HereDocumentStripTabs
            ) {
                let id = redirect
                    .here_document
                    .ok_or_else(|| anyhow::anyhow!("Missing here-document"))?;
                let ast = self.ast;
                let content = self.scalar(
                    &ast.here_documents
                        .get(id)
                        .ok_or_else(|| anyhow::anyhow!("Invalid here-document"))?
                        .content,
                    &io,
                )?;
                io.0.insert(fd, io::input(&content)?);
                continue;
            }
            let target = if redirect.operator == R::HereString {
                self.scalar(&redirect.target, &io)?
            } else {
                let targets = self.words(std::slice::from_ref(&redirect.target), &io)?;
                if targets.len() != 1 {
                    bail!("Ambiguous redirect");
                }
                targets.into_iter().next().unwrap()
            };
            match redirect.operator {
                R::CloseInput | R::CloseOutput => {
                    io.0.remove(&fd);
                }
                R::DuplicateInput | R::DuplicateOutput => {
                    if target == "-" {
                        io.0.remove(&fd);
                    } else {
                        let moving = target.ends_with('-');
                        let source = target.trim_end_matches('-').parse::<u32>()?;
                        let file = io.get(source)?;
                        io.0.insert(fd, file);
                        if moving {
                            io.0.remove(&source);
                        }
                    }
                }
                R::HereString => {
                    io.0.insert(fd, io::input(&format!("{target}\n"))?);
                }
                R::Input
                | R::Output
                | R::Append
                | R::Clobber
                | R::ReadWrite
                | R::OutputAndError
                | R::AppendAndError => {
                    let mut options = OpenOptions::new();
                    match redirect.operator {
                        R::Input => {
                            options.read(true);
                        }
                        R::ReadWrite => {
                            options.read(true).write(true).create(true);
                        }
                        R::Append | R::AppendAndError => {
                            options.append(true).create(true);
                        }
                        _ => {
                            options.write(true).create(true).truncate(true);
                        }
                    }
                    let file = std::sync::Arc::new(
                        options
                            .open(self.path(&target))
                            .with_context(|| target.clone())?,
                    );
                    if matches!(redirect.operator, R::OutputAndError | R::AppendAndError) {
                        io.0.insert(1, file.clone());
                        io.0.insert(2, file);
                    } else {
                        io.0.insert(fd, file);
                    }
                }
                R::HereDocument | R::HereDocumentStripTabs => unreachable!(),
            }
        }
        Ok(io)
    }
}

fn loop_control(result: &mut Outcome) -> bool {
    match result.flow {
        Flow::Break(1) => {
            result.flow = Flow::Normal;
            true
        }
        Flow::Continue(1) => {
            result.flow = Flow::Normal;
            false
        }
        Flow::Break(n) => {
            result.flow = Flow::Break(n - 1);
            true
        }
        Flow::Continue(n) => {
            result.flow = Flow::Continue(n - 1);
            true
        }
        Flow::Normal => false,
        _ => true,
    }
}
