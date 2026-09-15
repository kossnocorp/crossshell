mod prelude;

mod cli;
pub use cli::*;

mod cmd;
use cmd::*;

fn main() {
    CshCli::main();
}
