use crate::prelude::*;

#[derive(Args, Debug)]
pub struct CshCmdRun {
    #[usage()]
    pub script: String,
}

impl Run for CshCmdRun {
    type Output = Result<()>;

    fn run(self) -> Self::Output {
        let source_code = std::fs::read_to_string(&self.script)
            .with_context(|| format!("Failed to read script {}", self.script))?;

        let ast = match CshAst::parse(&source_code) {
            Ok(ast) => ast,
            Err(error) => {
                for diagnostic in &error.errors {
                    let span = (self.script.as_str(), diagnostic.span().into_range());
                    Report::build(ReportKind::Error, span.clone())
                        .with_config(Config::default().with_index_type(IndexType::Byte))
                        .with_message(diagnostic.to_string())
                        .with_label(
                            Label::new(span)
                                .with_message(diagnostic.reason().to_string())
                                .with_color(Color::Red),
                        )
                        .finish()
                        .eprint((self.script.as_str(), Source::from(&source_code)))?;
                }
                return Err(error.into());
            }
        };

        println!("{ast:#?}");

        Ok(())
    }
}
