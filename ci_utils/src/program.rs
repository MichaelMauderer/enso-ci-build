use crate::prelude::*;

pub mod command;
mod location;
pub mod resolver;
pub mod shell;
pub mod version;
pub mod with_cwd;

pub use command::Command;
use location::Location;


use crate::program::command::MyCommand;
pub use resolver::Resolver;
pub use shell::Shell;

// TODO: consider project manger wrapper:
// TODO: separate locating (which might be stateful, e.g. with additional directories)
// TODO: separate "what can be done with its command" from the rest of program (e.g. from name)

pub const EMPTY_ARGS: [&str; 0] = [];
//
// pub trait Locator {
//     fn foo(&self) -> Result<PathBuf>;
// }
//
// pub struct DefaultLocator<P: Program> {}
//
// impl<P:Program> Locator {
//
// }


/// A set of utilities for using a known external program.
///
/// The trait covers program lookup and process management.
// `Sized + 'static` bounds are due to using `Self` as type parameter for `Command` constructor.
#[async_trait]
pub trait Program: Sized + 'static {
    type Command: MyCommand<Self> + Send + Sync = Command;

    /// The name used to find and invoke the program.
    ///
    /// This should just the stem name, not a full path. The os-specific executable extension should
    /// be skipped.
    fn executable_name() -> &'static str;

    /// If program can be found under more than one name, additional names are provided.
    ///
    /// The primary name is provided by ['executable_name'].
    fn executable_name_fallback() -> Vec<&'static str> {
        vec![]
    }

    fn default_locations(&self) -> Vec<PathBuf> {
        Vec::new()
    }

    fn pretty_name() -> &'static str {
        Self::executable_name()
    }

    /// Locate the program executable.
    ///
    /// The lookup locations are program-defined, they typically include Path environment variable
    /// and program-specific default locations.
    fn lookup(&self) -> anyhow::Result<Location<Self>> {
        Resolver::new(Self::executable_names(), self.default_locations())?
            .lookup()
            .map(Location::new)
    }

    async fn require_present(&self) -> Result<String> {
        let version = self.version_string().await?;
        debug!("Found {}: {}", Self::executable_name(), version);
        Ok(version)
    }

    async fn require_present_at(&self, required_version: &Version) -> Result {
        let found_version = self.require_present().await?;
        let found_version = self.parse_version(&found_version)?;
        if &found_version != required_version {
            bail!(
                "Failed to find {} in version == {}. Found version: {}",
                Self::executable_name(),
                required_version,
                found_version
            )
        }
        Ok(())
    }

    fn cmd(&self) -> Result<Self::Command> {
        let program_path = self.lookup()?;
        let mut command = Self::Command::new_program(program_path);
        if let Some(current_dir) = self.current_directory() {
            command.borrow_mut().current_dir(current_dir);
        }
        self.init_command(&mut command);
        Ok(command)
    }

    fn init_command<'a>(&self, cmd: &'a mut Self::Command) -> &'a mut Self::Command {
        cmd
    }

    fn current_directory(&self) -> Option<PathBuf> {
        None
    }

    fn handle_exit_status(status: std::process::ExitStatus) -> Result {
        status.exit_ok().anyhow_err()
    }

    /// Command that prints to stdout the version of given program.
    ///
    /// If this is anything other than `--version` the implementor should overwrite this method.
    fn version_command(&self) -> Result<Self::Command> {
        let mut cmd = self.cmd()?;
        cmd.borrow_mut().arg("--version");
        Ok(cmd)
    }

    async fn version_string(&self) -> Result<String> {
        let output = self.version_command()?.borrow_mut().run_stdout().await?;
        Ok(output.trim().to_string())
    }

    // TODO if such need appears, likely Version should be made an associated type
    async fn version(&self) -> Result<Version> {
        let stdout = self.version_string().await?;
        self.parse_version(&stdout)
    }

    /// Retrieve semver-compatible version from the string in format provided by the
    /// `version_string`.
    ///
    /// Some programs do not follow semver for versioning, for them this method is unspecified.
    fn parse_version(&self, version_text: &str) -> Result<Version> {
        version::find_in_text(version_text)
    }
}

pub trait ProgramExt: Program {
    fn executable_names() -> Vec<&'static str> {
        let mut ret = vec![Self::executable_name()];
        ret.extend(Self::executable_name_fallback());
        ret
    }

    fn args(&self, args: impl IntoIterator<Item: AsRef<OsStr>>) -> Result<Self::Command> {
        let mut cmd = self.cmd()?;
        cmd.borrow_mut().args(args);
        Ok(cmd)
    }

    fn call_arg(&self, arg: impl AsRef<OsStr>) -> BoxFuture<'static, Result> {
        self.call_args(once(arg))
    }

    // We cannot use async_trait for this, as we need to separate lifetime of the future from the
    // arguments' lifetimes.
    fn call_args(&self, args: impl IntoIterator<Item: AsRef<OsStr>>) -> BoxFuture<'static, Result> {
        let mut cmd = match self.args(args) {
            Ok(cmd) => cmd,
            e @ Err(_) => return ready(e.map(|_| ())).boxed(),
        };
        cmd.borrow_mut().run_ok().boxed()
    }
}

impl<T> ProgramExt for T where T: Program {}
