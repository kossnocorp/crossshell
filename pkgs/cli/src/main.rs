mod prelude;

mod cli;
pub use cli::*;

mod cmd;
pub use cmd::*;

fn main() {
    CshCli::main();
}
