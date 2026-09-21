use super::CshAstWord;
use std::borrow::Cow;
use std::ops::Range;

/// Arithmetic expression:
///     (( 1 + 3 ))
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstArithmetic<'a> {
    pub span: Range<usize>,
    pub kind: CshAstArithmeticKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstArithmeticKind<'a> {
    /// Preserve explicit parentheses, including when expansions inject operators.
    Group(Box<CshAstArithmetic<'a>>),
    /// Digits are retained to avoid host-dependent integer conversion at parse time.
    Number {
        radix: u32,
        digits: Cow<'a, str>,
    },
    Variable(Cow<'a, str>),
    /// Shell expansion may yield arithmetic tokens, not just a numeric value.
    /// Expand these before evaluating the resulting arithmetic expression.
    Expansion(Box<CshAstWord<'a>>),
    Subscript {
        array: Box<CshAstArithmetic<'a>>,
        index: Box<CshAstArithmetic<'a>>,
    },
    Unary {
        operator: CshAstArithmeticUnary,
        operand: Box<CshAstArithmetic<'a>>,
    },
    Binary {
        left: Box<CshAstArithmetic<'a>>,
        operator: CshAstArithmeticBinary,
        right: Box<CshAstArithmetic<'a>>,
    },
    Conditional {
        condition: Box<CshAstArithmetic<'a>>,
        then_value: Box<CshAstArithmetic<'a>>,
        else_value: Box<CshAstArithmetic<'a>>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstArithmeticUnary {
    Plus,
    Minus,
    Not,
    BitNot,
    PreIncrement,
    PreDecrement,
    PostIncrement,
    PostDecrement,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstArithmeticBinary {
    Comma,
    Assign,
    AddAssign,
    SubtractAssign,
    MultiplyAssign,
    DivideAssign,
    RemainderAssign,
    ShiftLeftAssign,
    ShiftRightAssign,
    BitAndAssign,
    BitXorAssign,
    BitOrAssign,
    Or,
    And,
    BitOr,
    BitXor,
    BitAnd,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    ShiftLeft,
    ShiftRight,
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Power,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstArithmeticFor<'a> {
    pub init: Option<CshAstArithmetic<'a>>,
    pub condition: Option<CshAstArithmetic<'a>>,
    pub update: Option<CshAstArithmetic<'a>>,
}

/// Conditional test:
///     [[ -n "$name" ]]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCondition<'a> {
    pub span: Range<usize>,
    pub kind: CshAstConditionKind<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstConditionKind<'a> {
    Word(CshAstWord<'a>),
    Unary {
        operator: CshAstTestUnary,
        operand: CshAstWord<'a>,
    },
    Binary {
        left: CshAstWord<'a>,
        operator: CshAstTestBinary,
        right: CshAstWord<'a>,
    },
    Not(Box<CshAstCondition<'a>>),
    And(Box<CshAstCondition<'a>>, Box<CshAstCondition<'a>>),
    Or(Box<CshAstCondition<'a>>, Box<CshAstCondition<'a>>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstTestUnary {
    Exists,
    Block,
    Character,
    Directory,
    Regular,
    SetGroupId,
    Symlink,
    Sticky,
    Fifo,
    Readable,
    NonemptyFile,
    Terminal,
    SetUserId,
    Writable,
    Executable,
    OwnedByUser,
    OwnedByGroup,
    ModifiedSinceRead,
    Socket,
    OptionEnabled,
    VariableSet,
    NameReference,
    EmptyString,
    NonemptyString,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstTestBinary {
    PatternEqual,
    PatternNotEqual,
    Regex,
    StringLess,
    StringGreater,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    Newer,
    Older,
    SameFile,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstParameter<'a> {
    pub name: &'a str,
    pub subscript: Option<CshAstWord<'a>>,
    pub mode: CshAstParameterMode,
    pub operation: CshAstParameterOperation<'a>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstParameterMode {
    Value,
    Length,
    Indirect,
    Names { separate: bool },
    Indices { separate: bool },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstParameterOperation<'a> {
    None,
    Default {
        operator: CshAstDefaultOperator,
        test_empty: bool,
        word: CshAstWord<'a>,
    },
    Slice {
        offset: CshAstArithmetic<'a>,
        length: Option<CshAstArithmetic<'a>>,
    },
    Trim {
        suffix: bool,
        longest: bool,
        pattern: CshAstWord<'a>,
    },
    Replace {
        anchor: CshAstReplaceAnchor,
        pattern: CshAstWord<'a>,
        replacement: CshAstWord<'a>,
    },
    Case {
        upper: bool,
        all: bool,
        pattern: CshAstWord<'a>,
    },
    Transform(CshAstParameterTransform),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstDefaultOperator {
    Use,
    Assign,
    Error,
    Alternate,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstReplaceAnchor {
    First,
    All,
    Prefix,
    Suffix,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstParameterTransform {
    Quote,
    Escape,
    Prompt,
    Assignment,
    Attributes,
    Upper,
    UpperFirst,
    Lower,
    KeyValues,
    Words,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBraceSequence<'a> {
    pub start: &'a str,
    pub end: &'a str,
    pub step: Option<&'a str>,
    pub alphabetic: bool,
    pub padding: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstTilde<'a> {
    Home,
    User(&'a str),
    WorkingDirectory,
    PreviousDirectory,
    DirectoryStack {
        index: &'a str,
        reverse: bool,
        explicit_sign: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstDescriptor<'a> {
    Default,
    Number(&'a str),
    Variable(&'a str),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CshAstRedirectOperator {
    Input,
    Output,
    Append,
    Clobber,
    ReadWrite,
    DuplicateInput,
    DuplicateOutput,
    CloseInput,
    CloseOutput,
    HereDocument,
    HereDocumentStripTabs,
    HereString,
    OutputAndError,
    AppendAndError,
}
