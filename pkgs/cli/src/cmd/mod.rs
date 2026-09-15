use crate::prelude::*;

mod run;
use run::*;

#[derive(Subcommands)]
#[usage(run)]
pub enum CshCmd {
    /// Runs a shell program
    Run(CshCmdRun),
}
