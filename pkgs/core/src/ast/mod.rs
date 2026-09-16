mod debug;

/// An owned syntax tree. Expression links are stable indices into `nodes`.
/// Moving the tree or growing the arena never invalidates a node ID.
#[derive(Clone, PartialEq, Eq)]
pub struct CshAst {
    pub commands: CshAstList,
    pub nodes: Vec<CshAstExpression>,
    pub here_documents: Vec<CshAstHereDocument>,
}

/// An expression index in the owning `CshAst::nodes` arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CshAstNodeId(pub usize);

pub type CshAstList = Vec<CshAstNodeId>;

impl std::ops::Index<CshAstNodeId> for CshAst {
    type Output = CshAstExpression;

    fn index(&self, id: CshAstNodeId) -> &Self::Output {
        &self.nodes[id.0]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstExpression {
    Command(CshAstCommand),
    Function {
        name: String,
        body: CshAstNodeId,
    },
    /// Conditional syntax with structured expansions and quotes. Operators and
    /// regex text are literal fragments, rather than command-list operators.
    Test(CshAstWord),
    /// Arithmetic syntax is retained without evaluating it.
    Arithmetic(String),
    ArithmeticFor {
        clauses: String,
        body: CshAstList,
    },
    Binary {
        left: CshAstNodeId,
        operator: CshAstOperator,
        right: CshAstNodeId,
    },
    Background(CshAstNodeId),
    Subshell(CshAstList),
    Group(CshAstList),
    Negated(CshAstNodeId),
    If {
        branches: Vec<CshAstBranch>,
        otherwise: Option<CshAstList>,
    },
    For {
        variable: String,
        words: Option<Vec<CshAstWord>>,
        body: CshAstList,
    },
    Loop {
        until: bool,
        condition: CshAstList,
        body: CshAstList,
    },
    Case {
        word: CshAstWord,
        arms: Vec<CshAstCaseArm>,
    },
    Redirected {
        expression: CshAstNodeId,
        redirects: Vec<CshAstRedirect>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBranch {
    pub condition: CshAstList,
    pub body: CshAstList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCaseArm {
    pub patterns: Vec<CshAstWord>,
    pub body: CshAstList,
    pub terminator: String,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CshAstRedirect {
    /// An explicit descriptor number, or `{name}` for Bash descriptor allocation
    /// (and variable-selected closing with `<&-`/`>&-`). Braces are retained to
    /// distinguish the variable form; `None` selects the operator's default.
    pub descriptor: Option<String>,
    pub operator: String,
    pub target: CshAstWord,
    /// Index into the owning AST's here-document arena.
    pub here_document: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstHereDocument {
    pub delimiter: String,
    pub quoted: bool,
    pub strip_tabs: bool,
    pub body: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstOperator {
    Pipe,
    PipeWithStderr,
    And,
    Or,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCommand {
    pub assignments: Vec<CshAstAssignment>,
    pub name: Option<CshAstWord>,
    pub args: Vec<CshAstWord>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstAssignment {
    pub name: String,
    pub operator: CshAstAssignmentOperator,
    pub value: CshAstWord,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstAssignmentOperator {
    Set,
    Append,
}

/// One shell word. Concatenated fragments remain a single argument; quoting and
/// escaping are preserved so an evaluator can decide splitting and globbing.
/// Substitution command IDs refer to the owning `CshAst::nodes` arena.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstWord {
    Literal(String),
    SingleQuoted(String),
    /// ANSI-C quoted content, before interpreting backslash escapes.
    AnsiCQuoted(String),
    DoubleQuoted(Box<CshAstWord>),
    LocaleQuoted(Box<CshAstWord>),
    Escaped(String),
    Variable(String),
    /// Braced parameter syntax, including structured expansions in the suffix.
    Parameter {
        prefix: String,
        name: String,
        suffix: Box<CshAstWord>,
    },
    CommandSubstitution {
        commands: CshAstList,
        backticks: bool,
    },
    ProcessSubstitution {
        operator: String,
        commands: CshAstList,
    },
    ArithmeticExpansion(Box<CshAstWord>),
    Concat(Vec<CshAstWord>),
    Array(Vec<CshAstWord>),
    /// An assignment argument of a declaration builtin, retaining argument order.
    Assignment(Box<CshAstAssignment>),
    /// A keyed entry inside a compound array assignment, not a glob pattern.
    KeyedElement {
        key: Box<CshAstWord>,
        operator: CshAstAssignmentOperator,
        value: Box<CshAstWord>,
    },
    /// Unquoted glob syntax (quoted wildcard characters remain literals).
    Glob(CshAstGlob),
    ExtendedGlob {
        operator: char,
        alternatives: Vec<CshAstWord>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstGlob {
    /// `*`: zero or more characters; pathname matching does not cross `/`.
    Star,
    /// `**`: double-star syntax. Recursive pathname matching depends on the
    /// evaluator's globstar option and the token's position in the pattern.
    GlobStar,
    /// `?`: one character; pathname matching excludes `/`.
    QuestionMark,
    CharacterClass {
        negated: bool,
        items: Vec<CshAstGlobClassItem>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstGlobClassItem {
    Character(char),
    Range { start: char, end: char },
    NamedClass(String),
    CollatingSymbol(String),
    EquivalenceClass(String),
}

impl CshAstWord {
    pub(crate) fn concat(mut parts: Vec<Self>) -> Self {
        match parts.len() {
            0 => Self::Literal(String::new()),
            1 => parts.pop().unwrap(),
            _ => Self::Concat(parts),
        }
    }
}
