use super::CshAstWord;
use std::ops::Range;

/// Arithmetic expression:
///     (( 1 + 3 ))
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstArithmetic {
    pub span: Range<usize>,
    pub kind: CshAstArithmeticKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstArithmeticKind {
    /// Preserve explicit parentheses, including when expansions inject operators.
    Group(Box<CshAstArithmetic>),
    /// Digits are retained to avoid host-dependent integer conversion at parse time.
    Number {
        radix: u32,
        digits: String,
    },
    Variable(String),
    /// Shell expansion may yield arithmetic tokens, not just a numeric value.
    /// Expand these before evaluating the resulting arithmetic expression.
    Expansion(Box<CshAstWord>),
    Subscript {
        array: Box<CshAstArithmetic>,
        index: Box<CshAstArithmetic>,
    },
    Unary {
        operator: CshAstArithmeticUnary,
        operand: Box<CshAstArithmetic>,
    },
    Binary {
        left: Box<CshAstArithmetic>,
        operator: CshAstArithmeticBinary,
        right: Box<CshAstArithmetic>,
    },
    Conditional {
        condition: Box<CshAstArithmetic>,
        then_value: Box<CshAstArithmetic>,
        else_value: Box<CshAstArithmetic>,
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
pub struct CshAstArithmeticFor {
    pub init: Option<CshAstArithmetic>,
    pub condition: Option<CshAstArithmetic>,
    pub update: Option<CshAstArithmetic>,
}

/// Conditional test:
///     [[ -n "$name" ]]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCondition {
    pub span: Range<usize>,
    pub kind: CshAstConditionKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstConditionKind {
    Word(CshAstWord),
    Unary {
        operator: CshAstTestUnary,
        operand: CshAstWord,
    },
    Binary {
        left: CshAstWord,
        operator: CshAstTestBinary,
        right: CshAstWord,
    },
    Not(Box<CshAstCondition>),
    And(Box<CshAstCondition>, Box<CshAstCondition>),
    Or(Box<CshAstCondition>, Box<CshAstCondition>),
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
pub struct CshAstParameter {
    pub name: String,
    pub subscript: Option<CshAstWord>,
    pub mode: CshAstParameterMode,
    pub operation: CshAstParameterOperation,
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
pub enum CshAstParameterOperation {
    None,
    Default {
        operator: CshAstDefaultOperator,
        test_empty: bool,
        word: CshAstWord,
    },
    Slice {
        offset: CshAstArithmetic,
        length: Option<CshAstArithmetic>,
    },
    Trim {
        suffix: bool,
        longest: bool,
        pattern: CshAstWord,
    },
    Replace {
        anchor: CshAstReplaceAnchor,
        pattern: CshAstWord,
        replacement: CshAstWord,
    },
    Case {
        upper: bool,
        all: bool,
        pattern: CshAstWord,
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
pub struct CshAstBraceSequence {
    pub start: String,
    pub end: String,
    pub step: Option<String>,
    pub alphabetic: bool,
    pub padding: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstTilde {
    Home,
    User(String),
    WorkingDirectory,
    PreviousDirectory,
    DirectoryStack {
        index: String,
        reverse: bool,
        explicit_sign: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstDescriptor {
    Default,
    Number(String),
    Variable(String),
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
