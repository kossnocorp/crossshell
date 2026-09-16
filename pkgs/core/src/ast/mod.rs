#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAst {
    pub commands: Vec<CshAstExpression>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CshAstExpression {
    Command(CshAstCommand),
    Binary {
        left: Box<Self>,
        operator: CshAstOperator,
        right: Box<Self>,
    },
    Background(Box<Self>),
    Subshell(CshAst),
    Group(CshAst),
    Negated(Box<Self>),
    If {
        branches: Vec<CshAstBranch>,
        otherwise: Option<CshAst>,
    },
    For {
        variable: String,
        words: Option<Vec<String>>,
        body: CshAst,
    },
    Loop {
        until: bool,
        condition: CshAst,
        body: CshAst,
    },
    Case {
        word: String,
        arms: Vec<CshAstCaseArm>,
    },
    Redirected {
        expression: Box<Self>,
        redirects: Vec<CshAstRedirect>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstBranch {
    pub condition: CshAst,
    pub body: CshAst,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCaseArm {
    pub patterns: Vec<String>,
    pub body: CshAst,
    pub terminator: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstRedirect {
    pub descriptor: Option<String>,
    pub operator: String,
    pub target: String,
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
    pub name: String,
    pub args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstAssignment {
    pub name: String,
    pub value: String,
}
