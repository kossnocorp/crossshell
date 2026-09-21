mod debug;
use std::borrow::Cow;

mod syntax;
pub use syntax::*;

/// A syntax tree borrowing its source. Expression links are stable indices into `nodes`.
/// Moving the tree or growing the arena never invalidates a node ID.
#[derive(Clone, PartialEq, Eq)]
pub struct CshAst<'a> {
    /// Original source; text fragments borrow this same caller-owned buffer.
    pub source: &'a str,
    pub commands: CshAstList,
    pub nodes: Vec<CshAstExpression<'a>>,
    /// UTF-8 source byte ranges, indexed by the corresponding expression ID.
    pub spans: Vec<std::ops::Range<usize>>,
    pub here_documents: Vec<CshAstHereDocument<'a>>,
    /// Comments from all nesting levels, in source order, when requested by the parser.
    /// These are syntax metadata, not executable expressions. Use their spans to
    /// relate them to expressions, including comments before closing delimiters.
    pub comments: Vec<CshAstComment<'a>>,
}

/// A shell comment, including shebangs and empty comments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstComment<'a> {
    /// UTF-8 source byte range including `#`, excluding the terminating newline.
    pub span: std::ops::Range<usize>,
    /// Source text after `#`, without the terminating newline. Whitespace is preserved.
    pub text: &'a str,
}

/// An expression index in the owning `CshAst::nodes` arena.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CshAstNodeId(pub usize);

pub type CshAstList = Vec<CshAstNodeId>;

impl<'a> std::ops::Index<CshAstNodeId> for CshAst<'a> {
    type Output = CshAstExpression<'a>;

    fn index(&self, id: CshAstNodeId) -> &Self::Output {
        &self.nodes[id.0]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstExpression<'a> {
    /// Command:
    ///     printf '%s\n' hello
    Command(CshAstCommand<'a>),

    /// Function definition:
    ///     greet() {
    ///         echo hello
    ///     }
    Function(CshAstFunction<'a>),

    /// Conditional test:
    ///     [[ -n "$name" ]]
    Test(CshAstCondition<'a>),

    /// Arithmetic expression:
    ///     (( 1 + 3 ))
    ///     (( 2 * 5 ))
    Arithmetic(CshAstArithmetic<'a>),

    /// Arithmetic `for` loop:
    ///     for ((i = 0; i < 3; i++)); do
    ///         echo "$i"
    ///     done
    ArithmeticFor(CshAstArithmeticForExpression<'a>),

    /// Commands combined with a pipeline or logical operator:
    ///     printf '%s\n' hello | grep hello
    Binary(CshAstBinary),

    /// Command running in the background:
    ///     sleep 1 &
    Background(CshAstBackground),

    /// Commands running in a subshell:
    ///     (
    ///         cd /tmp
    ///         pwd
    ///     )
    Subshell(CshAstSubshell),

    /// Grouped list of commands:
    ///     {
    ///         echo one
    ///         echo two
    ///     }
    Group(CshAstGroup),

    /// Negated command:
    ///     ! test -f missing.txt
    Negated(CshAstNegated),

    /// Conditional branches:
    ///     if test -f file; then
    ///         echo yes
    ///     else
    ///         echo no
    ///     fi
    If(CshAstIf),

    /// Word-based `for` loop:
    ///     for file in *.txt; do
    ///         echo "$file"
    ///     done
    For(CshAstFor<'a>),

    /// `while` or `until` loop:
    ///     while test "$n" -lt 3; do
    ///         echo "$n"
    ///     done
    Loop(CshAstLoop),

    /// Pattern-matched branches:
    ///     case "$answer" in
    ///         y) echo yes ;;
    ///         n) echo no ;;
    ///     esac
    Case(CshAstCase<'a>),

    /// Command with input or output redirection:
    ///     cat < input.txt > output.txt
    Redirected(CshAstRedirected<'a>),
}

/// Function definition:
///     greet() {
///         echo hello
///     }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstFunction<'a> {
    pub name: &'a str,
    pub body: CshAstNodeId,
}

/// Arithmetic `for` loop:
///     for ((i = 0; i < 3; i++)); do
///         echo "$i"
///     done
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstArithmeticForExpression<'a> {
    pub clauses: CshAstArithmeticFor<'a>,
    pub body: CshAstList,
}

/// Commands combined with a pipeline or logical operator:
///     printf '%s\n' hello | grep hello
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBinary {
    pub left: CshAstNodeId,
    pub operator: CshAstOperator,
    pub right: CshAstNodeId,
}

/// Conditional branches:
///     if test -f file; then
///         echo yes
///     else
///         echo no
///     fi
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstIf {
    pub branches: Vec<CshAstBranch>,
    pub otherwise: Option<CshAstList>,
}

/// Word-based `for` loop:
///     for file in *.txt; do
///         echo "$file"
///     done
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstFor<'a> {
    pub variable: Cow<'a, str>,
    pub words: Option<Vec<CshAstWord<'a>>>,
    pub body: CshAstList,
}

/// `while` or `until` loop:
///     while test "$n" -lt 3; do
///         echo "$n"
///     done
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstLoop {
    pub until: bool,
    pub condition: CshAstList,
    pub body: CshAstList,
}

/// Pattern-matched branches:
///     case "$answer" in
///         y) echo yes ;;
///         n) echo no ;;
///     esac
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCase<'a> {
    pub word: CshAstWord<'a>,
    pub arms: Vec<CshAstCaseArm<'a>>,
}

/// Command with input or output redirection:
///     cat < input.txt > output.txt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstRedirected<'a> {
    pub expression: CshAstNodeId,
    pub redirects: Vec<CshAstRedirect<'a>>,
}

/// Command running in the background:
///     sleep 1 &
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBackground {
    pub expression: CshAstNodeId,
}

/// Commands running in a subshell:
///     (
///         cd /tmp
///         pwd
///     )
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstSubshell {
    pub body: CshAstList,
}

/// Grouped list of commands:
///     {
///         echo one
///         echo two
///     }
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstGroup {
    pub body: CshAstList,
}

/// Negated command:
///     ! test -f missing.txt
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstNegated {
    pub expression: CshAstNodeId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBranch {
    pub condition: CshAstList,
    pub body: CshAstList,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCaseArm<'a> {
    pub patterns: Vec<CshAstWord<'a>>,
    pub body: CshAstList,
    pub terminator: &'a str,
}

#[derive(Clone, PartialEq, Eq)]
pub struct CshAstRedirect<'a> {
    /// Default, numbered, or variable-allocated descriptor (`{name}`).
    pub descriptor: CshAstDescriptor<'a>,
    pub operator: CshAstRedirectOperator,
    pub target: CshAstWord<'a>,
    pub span: std::ops::Range<usize>,
    /// Index into the owning AST's here-document arena.
    pub here_document: Option<usize>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstHereDocument<'a> {
    pub delimiter: Cow<'a, str>,
    pub quoted: bool,
    pub strip_tabs: bool,
    pub body: Cow<'a, str>,
    /// Expanded with here-document rules, not ordinary shell-word rules.
    /// Quoted delimiters always produce a literal body.
    pub content: CshAstWord<'a>,
    pub span: std::ops::Range<usize>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstOperator {
    Pipe,
    PipeWithStderr,
    And,
    Or,
}

/// Command:
///     printf '%s\n' hello
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCommand<'a> {
    pub assignments: Vec<CshAstAssignment<'a>>,
    pub name: Option<CshAstWord<'a>>,
    pub args: Vec<CshAstWord<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstAssignment<'a> {
    pub name: &'a str,
    pub operator: CshAstAssignmentOperator,
    pub value: CshAstWord<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstAssignmentOperator {
    Set,
    Append,
}

/// One shell word. Concatenated fragments remain a single argument; quoting and
/// escaping are preserved so an evaluator can decide splitting and globbing.
/// Substitution command IDs refer to the owning `CshAst::nodes` arena.
/// Text borrows the input wherever possible. Literals use `Cow` because merging
/// fragments (for example after removing here-document continuations) can
/// produce text that is not a contiguous slice of the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstWord<'a> {
    Literal(Cow<'a, str>),
    SingleQuoted(&'a str),
    /// ANSI-C quoted content, before interpreting backslash escapes.
    AnsiCQuoted(&'a str),
    DoubleQuoted(Box<CshAstWord<'a>>),
    LocaleQuoted(Box<CshAstWord<'a>>),
    Escaped(&'a str),
    Variable(&'a str),
    /// Parameter selection, subscript, and a typed operation with expansion operands.
    Parameter(Box<CshAstParameter<'a>>),
    CommandSubstitution {
        commands: CshAstList,
        backticks: bool,
    },
    ProcessSubstitution {
        operator: &'a str,
        commands: CshAstList,
    },
    ArithmeticExpansion(Box<CshAstArithmetic<'a>>),
    BraceAlternatives(Vec<CshAstWord<'a>>),
    BraceSequence(CshAstBraceSequence<'a>),
    /// An unquoted tilde prefix. Eligibility is checked after brace expansion:
    /// start of a resulting word, or an unquoted colon in an assignment value.
    Tilde(CshAstTilde<'a>),
    Concat(Vec<CshAstWord<'a>>),
    Array(Vec<CshAstWord<'a>>),
    /// An assignment argument of a declaration builtin, retaining argument order.
    Assignment(Box<CshAstAssignment<'a>>),
    /// A keyed entry inside a compound array assignment, not a glob pattern.
    KeyedElement {
        key: Box<CshAstWord<'a>>,
        operator: CshAstAssignmentOperator,
        value: Box<CshAstWord<'a>>,
    },
    /// Unquoted glob syntax (quoted wildcard characters remain literals).
    Glob(CshAstGlob<'a>),
    ExtendedGlob {
        operator: char,
        alternatives: Vec<CshAstWord<'a>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstGlob<'a> {
    /// `*`: zero or more characters; pathname matching does not cross `/`.
    Star,
    /// `**`: double-star syntax. Recursive pathname matching depends on the
    /// evaluator's globstar option and the token's position in the pattern.
    GlobStar,
    /// `?`: one character; pathname matching excludes `/`.
    QuestionMark,
    CharacterClass {
        negated: bool,
        items: Vec<CshAstGlobClassItem<'a>>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstGlobClassItem<'a> {
    Character(char),
    Range { start: char, end: char },
    NamedClass(&'a str),
    CollatingSymbol(&'a str),
    EquivalenceClass(&'a str),
}

impl CshAstWord<'_> {
    pub(crate) fn concat(mut parts: Vec<Self>) -> Self {
        // Merge in place: most words have one fragment, so allocating a second
        // vector here would cost an allocation for every ordinary word.
        parts.dedup_by(|next, previous| {
            if let (Self::Literal(text), Self::Literal(previous)) = (next, previous) {
                previous.to_mut().push_str(text);
                true
            } else {
                false
            }
        });
        match parts.len() {
            0 => Self::Literal("".into()),
            1 => parts.pop().unwrap(),
            _ => Self::Concat(parts),
        }
    }
}
