use crate::prelude::*;

mod ast;
use ast::*;
mod run;
use run::*;

#[derive(Subcommands)]
#[usage(run)]
pub enum CshCmd {
    /// Parses a shell program and prints its AST
    #[usage(hide)]
    Ast(CshCmdAst),
    /// Runs a shell program
    Run(CshCmdRun),
}
