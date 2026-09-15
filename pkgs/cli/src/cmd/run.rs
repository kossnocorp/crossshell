use crate::prelude::*;

#[derive(Args, Debug)]
pub struct CshCmdRun {
    #[usage()]
    pub script: String,
}

impl Run for CshCmdRun {
    type Output = Result<()>;

    fn run(self) -> Self::Output {
        Ok(())
    }
}
