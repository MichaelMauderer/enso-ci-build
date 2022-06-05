use crate::prelude::*;

use crate::engine::BuildConfigurationFlags;
use crate::engine::BuildOperation;
use crate::project::IsArtifact;
use crate::project::IsTarget;

use anyhow::Context;
use ide_ci::goodie::GoodieDatabase;
use ide_ci::ok_ready_boxed;

pub use crate::project::backend::BuildInput;

#[derive(Clone, Debug)]
pub struct Artifact {
    pub root: PathBuf,
}

impl AsRef<Path> for Artifact {
    fn as_ref(&self) -> &Path {
        &self.root
    }
}

impl IsArtifact for Artifact {}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectManager;

impl IsTarget for ProjectManager {
    type BuildInput = BuildInput;
    type Artifact = Artifact;

    fn artifact_name(&self) -> String {
        "Enso Project Manager".into()
    }

    fn adapt_artifact(self, path: impl AsRef<Path>) -> BoxFuture<'static, Result<Self::Artifact>> {
        ok_ready_boxed(Artifact { root: path.as_ref().into() })
    }

    fn build_locally(
        &self,
        input: Self::BuildInput,
        output_path: impl AsRef<Path> + Send + Sync + 'static,
    ) -> BoxFuture<'static, Result<Self::Artifact>> {
        let this = self.clone();
        async move {
            let paths = crate::paths::Paths::new_versions(&input.repo_root, input.versions)?;
            let context = crate::engine::context::RunContext {
                operation: crate::engine::Operation::Build(BuildOperation {}),
                goodies: GoodieDatabase::new()?,
                config: BuildConfigurationFlags {
                    clean_repo: false,
                    build_project_manager_package: true,
                    ..crate::engine::NIGHTLY
                }
                .into(),
                octocrab: input.octocrab.clone(),
                paths,
            };
            let artifacts = context.build().await?;
            let engine_distribution =
                artifacts.packages.project_manager.context("Missing Project Manager package!")?;
            ide_ci::fs::mirror_directory(&engine_distribution.dir, &output_path).await?;
            this.adapt_artifact(output_path).await
        }
        .boxed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::setup_octocrab;
    use crate::version::Versions;
    use ide_ci::log::setup_logging;

    #[tokio::test]
    async fn build_project_manager() -> Result {
        setup_logging()?;
        let engine = ProjectManager;
        let input = BuildInput {
            versions:  Versions::default(),
            repo_root: r"H:\NBO\enso".into(),
            octocrab:  setup_octocrab().await?,
        };
        let output_path = r"C:\temp\project-manager";
        let result = engine.build_locally(input, output_path).await?;
        dbg!(&result);
        Ok(())
    }
}
