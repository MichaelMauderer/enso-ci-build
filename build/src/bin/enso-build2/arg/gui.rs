use crate::prelude::*;

use crate::arg::Source;
use crate::source_args_hlp;
use clap::Args;
use clap::Subcommand;
use enso_build::project::gui::Gui;
use enso_build::project::wasm::Wasm;

source_args_hlp!(Gui, "gui", BuildInput);

#[derive(Args, Clone, Debug, PartialEq)]
pub struct BuildInput {
    #[clap(flatten)]
    pub wasm: crate::arg::Source<Wasm>,
}

#[derive(Subcommand, Clone, Debug, PartialEq)]
pub enum Command {
    // Build {
    //     #[clap(flatten)]
    //     params:      BuildInput,
    //     #[clap(flatten)]
    //     output_path: OutputPath<Gui>,
    // },
    Get {
        #[clap(flatten)]
        source: Source<Gui>,
    },
    Watch {
        #[clap(flatten)]
        wasm: Source<Wasm>,
    },
}

#[derive(Args, Clone, Debug)]
pub struct Target {
    /// Command for GUI package.
    #[clap(subcommand)]
    pub command: Command,
}
