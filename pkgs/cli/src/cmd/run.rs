use crate::prelude::*;
use crossshell_interpreter::CshInterpreter;
use std::io::IsTerminal;

#[derive(Args, Debug)]
pub struct CshCmdRun {
    #[usage()]
    pub script: String,
}

impl Run for CshCmdRun {
    type Output = Result<()>;

    fn run(self) -> Self::Output {
        let source = std::fs::read_to_string(&self.script)
            .with_context(|| format!("Failed to read script {}", self.script))?;
        let ast = match CshParser::parse(&source) {
            Ok(ast) => ast,
            Err(error) => {
                let stderr = std::io::stderr();
                CshErrorReport::new(&source, &self.script, &error.errors, stderr.is_terminal())
                    .write(stderr.lock())?;
                std::process::exit(2);
            }
        };
        let mut interpreter = CshInterpreter::new(std::env::current_exe()?)?;
        interpreter.set_args(&self.script, Vec::new());
        let status = interpreter.run_ast(&ast)?;
        // Drop temporary executable links before exiting.
        drop(interpreter);
        if status != 0 {
            std::process::exit(i32::from(status));
        }
        Ok(())
    }
}
