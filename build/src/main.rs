use enso_build::prelude::*;

use enso_build::args::Args;
use enso_build::engine::RunContext;
use ide_ci::log::setup_logging;


#[tokio::main]
async fn main() -> Result {
    setup_logging()?;

    // We want arg parsing to be before any other logs, so if user types wrong arguments, the
    // error diagnostics will be first and only thing that is output.
    let args: Args = argh::from_env();

    debug!("Initial environment:");
    for (key, value) in std::env::vars() {
        debug!("\t{key}={value}");
    }
    debug!("\n===End of the environment dump===\n");

    let ctx = RunContext::new(&args).await?;
    ctx.execute().await?;
    Ok(())
}
