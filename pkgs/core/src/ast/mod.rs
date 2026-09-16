#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAst {
    pub commands: Vec<CshAstCommand>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CshAstCommand {
    pub name: String,
    pub args: Vec<String>,
}
