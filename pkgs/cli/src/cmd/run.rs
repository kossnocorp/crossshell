use super::ast::CshCmdAst;
use crate::prelude::*;

#[derive(Args, Debug)]
pub struct CshCmdRun {
    #[usage()]
    pub script: String,
}

impl Run for CshCmdRun {
    type Output = Result<()>;

    fn run(self) -> Self::Output {
        // Until evaluation is implemented, run prints the parsed program.
        CshCmdAst {
            script: self.script,
        }
        .run()
    }
}
