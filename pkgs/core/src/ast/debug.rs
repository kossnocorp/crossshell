//! Resolve arena links when formatting the tree, so debug output describes syntax.
use super::*;
use std::fmt::{self, Debug};
#[path = "debug_syntax.rs"]
mod syntax;
use syntax::*;

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
            debug.field("here_documents", &Documents(self));
        }
        if !self.comments.is_empty() {
            debug.field("comments", &self.comments);
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
            .field("patterns", &Words(self.0, &self.1.patterns))
            .field("body", &List(self.0, &self.1.body))
            .field("terminator", &self.1.terminator)
            .finish()
    }
}

impl Debug for Node<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let ast = self.0;
        match &ast[self.1] {
            CshAstExpression::Function(function) => f
                .debug_struct("Function")
                .field("name", &function.name)
                .field("body", &Node(ast, function.body))
                .finish(),
            CshAstExpression::Test(condition) => f
                .debug_tuple("Test")
                .field(&Condition(ast, condition))
                .finish(),
            CshAstExpression::Arithmetic(expression) => f
                .debug_tuple("Arithmetic")
                .field(&Arithmetic(ast, expression))
                .finish(),
            CshAstExpression::ArithmeticFor(expression) => f
                .debug_struct("ArithmeticFor")
                .field(
                    "init",
                    &expression.clauses.init.as_ref().map(|e| Arithmetic(ast, e)),
                )
                .field(
                    "condition",
                    &expression
                        .clauses
                        .condition
                        .as_ref()
                        .map(|e| Arithmetic(ast, e)),
                )
                .field(
                    "update",
                    &expression
                        .clauses
                        .update
                        .as_ref()
                        .map(|e| Arithmetic(ast, e)),
                )
                .field("body", &List(ast, &expression.body))
                .finish(),
            CshAstExpression::Command(command) => f
                .debug_tuple("Command")
                .field(&Command(ast, command))
                .finish(),
            CshAstExpression::Binary(binary) => f
                .debug_struct("Binary")
                .field("left", &Node(ast, binary.left))
                .field("operator", &binary.operator)
                .field("right", &Node(ast, binary.right))
                .finish(),
            CshAstExpression::Background(background) => f
                .debug_tuple("Background")
                .field(&Node(ast, background.expression))
                .finish(),
            CshAstExpression::Negated(negated) => f
                .debug_tuple("Negated")
                .field(&Node(ast, negated.expression))
                .finish(),
            CshAstExpression::Subshell(subshell) => f
                .debug_tuple("Subshell")
                .field(&List(ast, &subshell.body))
                .finish(),
            CshAstExpression::Group(group) => f
                .debug_tuple("Group")
                .field(&List(ast, &group.body))
                .finish(),
            CshAstExpression::If(expression) => f
                .debug_struct("If")
                .field("branches", &Branches(ast, &expression.branches))
                .field(
                    "otherwise",
                    &expression.otherwise.as_ref().map(|list| List(ast, list)),
                )
                .finish(),
            CshAstExpression::For(expression) => f
                .debug_struct("For")
                .field("variable", &expression.variable)
                .field(
                    "words",
                    &expression.words.as_ref().map(|words| Words(ast, words)),
                )
                .field("body", &List(ast, &expression.body))
                .finish(),
            CshAstExpression::Loop(expression) => f
                .debug_struct("Loop")
                .field("until", &expression.until)
                .field("condition", &List(ast, &expression.condition))
                .field("body", &List(ast, &expression.body))
                .finish(),
            CshAstExpression::Case(expression) => f
                .debug_struct("Case")
                .field("word", &Word(ast, &expression.word))
                .field("arms", &Arms(ast, &expression.arms))
                .finish(),
            CshAstExpression::Redirected(redirected) => f
                .debug_struct("Redirected")
                .field("expression", &Node(ast, redirected.expression))
                .field("redirects", &Redirects(ast, &redirected.redirects))
                .finish(),
        }
    }
}

struct Word<'a>(&'a CshAst, &'a CshAstWord);
struct Words<'a>(&'a CshAst, &'a [CshAstWord]);
struct Command<'a>(&'a CshAst, &'a CshAstCommand);
struct Assignment<'a>(&'a CshAst, &'a CshAstAssignment);
struct Assignments<'a>(&'a CshAst, &'a [CshAstAssignment]);
struct Redirect<'a>(&'a CshAst, &'a CshAstRedirect);
struct Redirects<'a>(&'a CshAst, &'a [CshAstRedirect]);

impl Debug for Word<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CshAstWord::*;
        match self.1 {
            Literal(s) => f.debug_tuple("Literal").field(s).finish(),
            SingleQuoted(s) => f.debug_tuple("SingleQuoted").field(s).finish(),
            AnsiCQuoted(s) => f.debug_tuple("AnsiCQuoted").field(s).finish(),
            Escaped(s) => f.debug_tuple("Escaped").field(s).finish(),
            Variable(s) => f.debug_tuple("Variable").field(s).finish(),
            Glob(glob) => f.debug_tuple("Glob").field(glob).finish(),
            DoubleQuoted(w) => f
                .debug_tuple("DoubleQuoted")
                .field(&Word(self.0, w))
                .finish(),
            LocaleQuoted(w) => f
                .debug_tuple("LocaleQuoted")
                .field(&Word(self.0, w))
                .finish(),
            ArithmeticExpansion(w) => f
                .debug_tuple("ArithmeticExpansion")
                .field(&Arithmetic(self.0, w))
                .finish(),
            BraceAlternatives(words) => f
                .debug_tuple("BraceAlternatives")
                .field(&Words(self.0, words))
                .finish(),
            BraceSequence(sequence) => f.debug_tuple("BraceSequence").field(sequence).finish(),
            Tilde(tilde) => f.debug_tuple("Tilde").field(tilde).finish(),
            Concat(w) => f.debug_tuple("Concat").field(&Words(self.0, w)).finish(),
            Array(w) => f.debug_tuple("Array").field(&Words(self.0, w)).finish(),
            Assignment(assignment) => f
                .debug_tuple("Assignment")
                .field(&self::Assignment(self.0, assignment))
                .finish(),
            KeyedElement {
                key,
                operator,
                value,
            } => f
                .debug_struct("KeyedElement")
                .field("key", &Word(self.0, key))
                .field("operator", operator)
                .field("value", &Word(self.0, value))
                .finish(),
            Parameter(parameter) => f
                .debug_struct("Parameter")
                .field("name", &parameter.name)
                .field(
                    "subscript",
                    &parameter.subscript.as_ref().map(|w| Word(self.0, w)),
                )
                .field("mode", &parameter.mode)
                .field(
                    "operation",
                    &ParameterOperation(self.0, &parameter.operation),
                )
                .finish(),
            ExtendedGlob {
                operator,
                alternatives,
            } => f
                .debug_struct("ExtendedGlob")
                .field("operator", operator)
                .field("alternatives", &Words(self.0, alternatives))
                .finish(),
            CommandSubstitution {
                commands,
                backticks,
            } => f
                .debug_struct("CommandSubstitution")
                .field("commands", &Nodes(self.0, commands))
                .field("backticks", backticks)
                .finish(),
            ProcessSubstitution { operator, commands } => f
                .debug_struct("ProcessSubstitution")
                .field("operator", operator)
                .field("commands", &Nodes(self.0, commands))
                .finish(),
        }
    }
}

impl Debug for Words<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|w| Word(self.0, w)))
            .finish()
    }
}
impl Debug for Command<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAstCommand")
            .field("assignments", &Assignments(self.0, &self.1.assignments))
            .field("name", &self.1.name.as_ref().map(|w| Word(self.0, w)))
            .field("args", &Words(self.0, &self.1.args))
            .finish()
    }
}
impl Debug for Assignments<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|a| Assignment(self.0, a)))
            .finish()
    }
}
impl Debug for Assignment<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAstAssignment")
            .field("name", &self.1.name)
            .field("operator", &self.1.operator)
            .field("value", &Word(self.0, &self.1.value))
            .finish()
    }
}
impl Debug for Redirects<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.1.iter().map(|r| Redirect(self.0, r)))
            .finish()
    }
}
impl Debug for Redirect<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let mut debug = f.debug_struct("CshAstRedirect");
        debug
            .field("descriptor", &self.1.descriptor)
            .field("operator", &self.1.operator)
            .field("target", &Word(self.0, &self.1.target));
        if let Some(id) = self.1.here_document {
            debug.field("here_document", &id);
        }
        debug.finish()
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
