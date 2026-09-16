use crate::prelude::*;
use std::io::{IsTerminal, Write};

#[derive(Args, Debug)]
pub struct CshCmdAst {
    #[usage()]
    pub script: String,
}

impl Run for CshCmdAst {
    type Output = Result<()>;

    fn run(self) -> Self::Output {
        let source_code = std::fs::read_to_string(&self.script)
            .with_context(|| format!("Failed to read script {}", self.script))?;

        let ast = match CshParser::parse(&source_code) {
            Ok(ast) => ast,

            Err(error) => {
                let stderr = std::io::stderr();
                CshErrorReport::new(
                    &source_code,
                    &self.script,
                    &error.errors,
                    stderr.is_terminal(),
                )
                .write(stderr.lock())?;
                anyhow::bail!("{error}");
            }
        };

        writeln!(std::io::stdout().lock(), "{ast:#?}")?;

        Ok(())
    }
}
