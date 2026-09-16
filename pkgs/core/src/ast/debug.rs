//! Resolve arena links when formatting the tree, so debug output describes syntax.
use super::*;
use std::fmt::{self, Debug};

struct List<'a>(&'a CshAst, &'a [CshAstNodeId]);
struct Nodes<'a>(&'a CshAst, &'a [CshAstNodeId]);
struct Node<'a>(&'a CshAst, CshAstNodeId);
struct Branch<'a>(&'a CshAst, &'a CshAstBranch);
struct Arm<'a>(&'a CshAst, &'a CshAstCaseArm);

impl Debug for CshAst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("CshAst");
        debug.field("commands", &Nodes(self, &self.commands));
        if !self.here_documents.is_empty() {
            debug.field("here_documents", &self.here_documents);
        }
        debug.finish()
    }
}

impl Debug for List<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAst")
            .field("commands", &Nodes(self.0, self.1))
            .finish()
    }
}

impl Debug for Nodes<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|id| Node(self.0, *id)))
            .finish()
    }
}

struct Branches<'a>(&'a CshAst, &'a [CshAstBranch]);
impl Debug for Branches<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|b| Branch(self.0, b)))
            .finish()
    }
}

struct Arms<'a>(&'a CshAst, &'a [CshAstCaseArm]);
impl Debug for Arms<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|a| Arm(self.0, a)))
            .finish()
    }
}

impl Debug for Branch<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAstBranch")
            .field("condition", &List(self.0, &self.1.condition))
            .field("body", &List(self.0, &self.1.body))
            .finish()
    }
}

impl Debug for Arm<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAstCaseArm")
            .field("patterns", &self.1.patterns)
            .field("body", &List(self.0, &self.1.body))
            .field("terminator", &self.1.terminator)
            .finish()
    }
}

impl Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ast = self.0;
        match &ast[self.1] {
            CshAstExpression::Function { name, body } => f
                .debug_struct("Function")
                .field("name", name)
                .field("body", &Node(ast, *body))
                .finish(),
            CshAstExpression::Test(text) => f.debug_tuple("Test").field(text).finish(),
            CshAstExpression::Arithmetic(text) => f.debug_tuple("Arithmetic").field(text).finish(),
            CshAstExpression::ArithmeticFor { clauses, body } => f
                .debug_struct("ArithmeticFor")
                .field("clauses", clauses)
                .field("body", &List(ast, body))
                .finish(),
            CshAstExpression::Command(command) => f.debug_tuple("Command").field(command).finish(),
            CshAstExpression::Binary {
                left,
                operator,
                right,
            } => f
                .debug_struct("Binary")
                .field("left", &Node(ast, *left))
                .field("operator", operator)
                .field("right", &Node(ast, *right))
                .finish(),
            CshAstExpression::Background(id) => {
                f.debug_tuple("Background").field(&Node(ast, *id)).finish()
            }
            CshAstExpression::Negated(id) => {
                f.debug_tuple("Negated").field(&Node(ast, *id)).finish()
            }
            CshAstExpression::Subshell(list) => {
                f.debug_tuple("Subshell").field(&List(ast, list)).finish()
            }
            CshAstExpression::Group(list) => {
                f.debug_tuple("Group").field(&List(ast, list)).finish()
            }
            CshAstExpression::If {
                branches,
                otherwise,
            } => f
                .debug_struct("If")
                .field("branches", &Branches(ast, branches))
                .field("otherwise", &otherwise.as_ref().map(|list| List(ast, list)))
                .finish(),
            CshAstExpression::For {
                variable,
                words,
                body,
            } => f
                .debug_struct("For")
                .field("variable", variable)
                .field("words", words)
                .field("body", &List(ast, body))
                .finish(),
            CshAstExpression::Loop {
                until,
                condition,
                body,
            } => f
                .debug_struct("Loop")
                .field("until", until)
                .field("condition", &List(ast, condition))
                .field("body", &List(ast, body))
                .finish(),
            CshAstExpression::Case { word, arms } => f
                .debug_struct("Case")
                .field("word", word)
                .field("arms", &Arms(ast, arms))
                .finish(),
            CshAstExpression::Redirected {
                expression,
                redirects,
            } => f
                .debug_struct("Redirected")
                .field("expression", &Node(ast, *expression))
                .field("redirects", redirects)
                .finish(),
        }
    }
}

impl Debug for CshAstRedirect {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("CshAstRedirect");
        debug
            .field("descriptor", &self.descriptor)
            .field("operator", &self.operator)
            .field("target", &self.target);
        if let Some(id) = self.here_document {
            debug.field("here_document", &id);
        }
        debug.finish()
    }
}
