mod debug;
mod syntax;
pub use syntax::*;

/// An owned syntax tree. Expression links are stable indices into `nodes`.
/// Moving the tree or growing the arena never invalidates a node ID.
#[derive(Clone, PartialEq, Eq)]
pub struct CshAst {
    pub commands: CshAstList,
    pub nodes: Vec<CshAstExpression>,
    /// UTF-8 source byte ranges, indexed by the corresponding expression ID.
    pub spans: Vec<std::ops::Range<usize>>,
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
    /// Conditional operators and precedence, with quote-aware word operands.
    Test(CshAstCondition),
    /// Arithmetic syntax is retained without evaluating it.
    Arithmetic(CshAstArithmetic),
    ArithmeticFor {
        clauses: CshAstArithmeticFor,
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
    /// Default, numbered, or variable-allocated descriptor (`{name}`).
    pub descriptor: CshAstDescriptor,
    pub operator: CshAstRedirectOperator,
    pub target: CshAstWord,
    pub span: std::ops::Range<usize>,
    /// Index into the owning AST's here-document arena.
    pub here_document: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstHereDocument {
    pub delimiter: String,
    pub quoted: bool,
    pub strip_tabs: bool,
    pub body: String,
    /// Expanded with here-document rules, not ordinary shell-word rules.
    /// Quoted delimiters always produce a literal body.
    pub content: CshAstWord,
    pub span: std::ops::Range<usize>,
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
    /// Parameter selection, subscript, and a typed operation with expansion operands.
    Parameter(Box<CshAstParameter>),
    CommandSubstitution {
        commands: CshAstList,
        backticks: bool,
    },
    ProcessSubstitution {
        operator: String,
        commands: CshAstList,
    },
    ArithmeticExpansion(Box<CshAstArithmetic>),
    BraceAlternatives(Vec<CshAstWord>),
    BraceSequence(CshAstBraceSequence),
    /// An unquoted tilde prefix. Eligibility is checked after brace expansion:
    /// start of a resulting word, or an unquoted colon in an assignment value.
    Tilde(CshAstTilde),
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
        let mut merged = Vec::new();
        for part in parts.drain(..) {
            if let Self::Literal(text) = &part
                && let Some(Self::Literal(previous)) = merged.last_mut()
            {
                previous.push_str(text);
            } else {
                merged.push(part);
            }
        }
        parts = merged;
        match parts.len() {
            0 => Self::Literal(String::new()),
            1 => parts.pop().unwrap(),
            _ => Self::Concat(parts),
        }
    }
}
