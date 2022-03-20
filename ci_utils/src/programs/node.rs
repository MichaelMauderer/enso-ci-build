use crate::prelude::*;
use crate::program::command::MyCommand;

#[derive(Clone, Copy, Debug, Default)]
pub struct Node;

impl Program for Node {
    fn executable_name() -> &'static str {
        "node"
    }
}

#[derive(Shrinkwrap)]
#[shrinkwrap(mutable)]
pub struct NpmCommand(pub Command);

impl MyCommand for NpmCommand {
    fn new_program<P: Program + 'static, S: AsRef<OsStr>>(program: S) -> Self {
        Self(Command::new_program::<P, S>(program))
    }
}

impl NpmCommand {
    pub fn install(&mut self) -> &mut Self {
        // // We must strip any UNC prefix, because CMD does not support having it as a current
        // // directory, and npm is effectively a CMD script wrapping the actual program. See:
        // // https://github.com/npm/cli/issues/3349
        // //
        // // If this becomes an issue, consider toggling `DisableUNCCheck` on win runner machines
        // and // revert this workaround. See also:
        // // https://www.ibm.com/support/pages/disableunccheck-registry-key-created-during-rational-synergy-installation
        // let path = dbg!(path.as_ref().strip_prefix(r"\\?\")).unwrap_or(path.as_ref());
        self.arg("install");
        self
    }
    pub fn workspace(&mut self, workspace: impl AsRef<OsStr>) -> &mut Self {
        self.arg("--workspace").arg(workspace);
        self
    }
    pub fn run(
        &mut self,
        script_name: impl AsRef<OsStr>,
        args: impl IntoIterator<Item: AsRef<OsStr>>,
    ) -> &mut Self {
        self.arg("run").arg(script_name).args(args);
        self
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct Npm;

impl Program for Npm {
    type Command = NpmCommand;

    fn executable_name() -> &'static str {
        "npm"
    }
}

impl Npm {
    pub fn install(&self, path: impl AsRef<Path>) -> anyhow::Result<NpmCommand> {
        // // We must strip any UNC prefix, because CMD does not support having it as a current
        // // directory, and npm is effectively a CMD script wrapping the actual program. See:
        // // https://github.com/npm/cli/issues/3349
        // //
        // // If this becomes an issue, consider toggling `DisableUNCCheck` on win runner machines
        // and // revert this workaround. See also:
        // // https://www.ibm.com/support/pages/disableunccheck-registry-key-created-during-rational-synergy-installation
        // let path = dbg!(path.as_ref().strip_prefix(r"\\?\")).unwrap_or(path.as_ref());
        let mut cmd = self.cmd()?;
        cmd.arg("install").current_dir(path);
        Ok(cmd)
    }
}
