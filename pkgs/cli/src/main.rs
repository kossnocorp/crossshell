mod prelude;

mod cli;
pub use cli::*;

mod cmd;
pub use cmd::*;

fn main() {
    if let Some(code) = crossshell_interpreter::dispatch_utility() {
        std::process::exit(code);
    }
    CshCli::main();
}
