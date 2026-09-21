use super::*;

pub(super) struct Arithmetic<'a>(pub &'a CshAst<'a>, pub &'a CshAstArithmetic<'a>);
pub(super) struct Condition<'a>(pub &'a CshAst<'a>, pub &'a CshAstCondition<'a>);
pub(super) struct ParameterOperation<'a>(pub &'a CshAst<'a>, pub &'a CshAstParameterOperation<'a>);
pub(super) struct Documents<'a>(pub &'a CshAst<'a>);
struct Document<'a>(&'a CshAst<'a>, &'a CshAstHereDocument<'a>);

impl Debug for Arithmetic<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CshAstArithmeticKind::*;
        let ast = self.0;
        match &self.1.kind {
            Group(inner) => f
                .debug_tuple("Group")
                .field(&Arithmetic(ast, inner))
                .finish(),
            Number { radix, digits } => f
                .debug_struct("Number")
                .field("radix", radix)
                .field("digits", digits)
                .finish(),
            Variable(name) => f.debug_tuple("Variable").field(name).finish(),
            Expansion(word) => f.debug_tuple("Expansion").field(&Word(ast, word)).finish(),
            Subscript { array, index } => f
                .debug_struct("Subscript")
                .field("array", &Arithmetic(ast, array))
                .field("index", &Arithmetic(ast, index))
                .finish(),
            Unary { operator, operand } => f
                .debug_struct("Unary")
                .field("operator", operator)
                .field("operand", &Arithmetic(ast, operand))
                .finish(),
            Binary {
                left,
                operator,
                right,
            } => f
                .debug_struct("Binary")
                .field("left", &Arithmetic(ast, left))
                .field("operator", operator)
                .field("right", &Arithmetic(ast, right))
                .finish(),
            Conditional {
                condition,
                then_value,
                else_value,
            } => f
                .debug_struct("Conditional")
                .field("condition", &Arithmetic(ast, condition))
                .field("then_value", &Arithmetic(ast, then_value))
                .field("else_value", &Arithmetic(ast, else_value))
                .finish(),
        }
    }
}

impl Debug for Condition<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CshAstConditionKind::*;
        let ast = self.0;
        match &self.1.kind {
            Word(word) => f
                .debug_tuple("Word")
                .field(&super::Word(ast, word))
                .finish(),
            Unary { operator, operand } => f
                .debug_struct("Unary")
                .field("operator", operator)
                .field("operand", &super::Word(ast, operand))
                .finish(),
            Binary {
                left,
                operator,
                right,
            } => f
                .debug_struct("Binary")
                .field("left", &super::Word(ast, left))
                .field("operator", operator)
                .field("right", &super::Word(ast, right))
                .finish(),
            Not(value) => f.debug_tuple("Not").field(&Condition(ast, value)).finish(),
            And(left, right) => f
                .debug_tuple("And")
                .field(&Condition(ast, left))
                .field(&Condition(ast, right))
                .finish(),
            Or(left, right) => f
                .debug_tuple("Or")
                .field(&Condition(ast, left))
                .field(&Condition(ast, right))
                .finish(),
        }
    }
}

impl Debug for ParameterOperation<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        use CshAstParameterOperation::*;
        let ast = self.0;
        match self.1 {
            None => f.write_str("None"),
            Default {
                operator,
                test_empty,
                word,
            } => f
                .debug_struct("Default")
                .field("operator", operator)
                .field("test_empty", test_empty)
                .field("word", &Word(ast, word))
                .finish(),
            Slice { offset, length } => f
                .debug_struct("Slice")
                .field("offset", &Arithmetic(ast, offset))
                .field("length", &length.as_ref().map(|e| Arithmetic(ast, e)))
                .finish(),
            Trim {
                suffix,
                longest,
                pattern,
            } => f
                .debug_struct("Trim")
                .field("suffix", suffix)
                .field("longest", longest)
                .field("pattern", &Word(ast, pattern))
                .finish(),
            Replace {
                anchor,
                pattern,
                replacement,
            } => f
                .debug_struct("Replace")
                .field("anchor", anchor)
                .field("pattern", &Word(ast, pattern))
                .field("replacement", &Word(ast, replacement))
                .finish(),
            Case {
                upper,
                all,
                pattern,
            } => f
                .debug_struct("Case")
                .field("upper", upper)
                .field("all", all)
                .field("pattern", &Word(ast, pattern))
                .finish(),
            Transform(transform) => f.debug_tuple("Transform").field(transform).finish(),
        }
    }
}

impl Debug for Documents<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entries(self.0.here_documents.iter().map(|d| Document(self.0, d)))
            .finish()
    }
}

impl Debug for Document<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("CshAstHereDocument")
            .field("delimiter", &self.1.delimiter)
            .field("quoted", &self.1.quoted)
            .field("strip_tabs", &self.1.strip_tabs)
            .field("content", &Word(self.0, &self.1.content))
            .finish()
    }
}
