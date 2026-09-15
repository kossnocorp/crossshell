use crate::prelude::*;

#[derive(Cli)]
#[usage(run, bin = "cssh", about = "Simple cross-platform shell language")]
pub struct CshCli {
    #[usage(subcommand)]
    pub command: CshCmd,
}

impl CshCli {
    pub fn main() {
        CshCli::parse().run().unwrap_or_else(|err| {
            eprintln!("Error: {:?}", err);
            std::process::exit(1);
        });
    }
}
