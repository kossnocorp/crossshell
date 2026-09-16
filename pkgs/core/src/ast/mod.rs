mod debug;

/// An owned syntax tree. Expression links are stable indices into `nodes`.
/// Moving the tree or growing the arena never invalidates a node ID.
#[derive(Clone, PartialEq, Eq)]
pub struct CshAst {
    pub commands: CshAstList,
    pub nodes: Vec<CshAstExpression>,
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
        words: Option<Vec<String>>,
        body: CshAstList,
    },
    Loop {
        until: bool,
        condition: CshAstList,
        body: CshAstList,
    },
    Case {
        word: String,
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
    pub patterns: Vec<String>,
    pub body: CshAstList,
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
