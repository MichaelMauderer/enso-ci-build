use crate::prelude::*;

use crate::engine::BuildConfiguration;
use crate::engine::BuildOperation;
use crate::project::IsArtifact;
use crate::project::IsTarget;
use crate::version::Versions;

use crate::paths::pretty_print_arch;
use anyhow::Context;
use ide_ci::archive::is_archive_name;
use ide_ci::goodie::GoodieDatabase;
use ide_ci::program::version::find_in_text;
use octocrab::models::repos::Asset;
use platforms::TARGET_ARCH;
use platforms::TARGET_OS;
use std::env::consts::EXE_SUFFIX;
use std::lazy::SyncLazy;

#[derive(Clone, Debug)]
pub struct BuildInput {
    pub repo_root: PathBuf,
    pub versions:  Versions,
    /// Necessary for GraalVM lookup.
    pub octocrab:  Octocrab,
}

#[derive(Clone, Debug)]
pub struct Artifact {
    pub path:     crate::paths::generated::ProjectManager,
    pub versions: Versions,
}

impl AsRef<Path> for Artifact {
    fn as_ref(&self) -> &Path {
        &self.path
    }
}

impl IsArtifact for Artifact {
    fn from_existing(path: impl AsRef<Path>) -> BoxFuture<'static, Result<Self>> {
        let path = crate::paths::generated::ProjectManager::new(path.as_ref(), EXE_SUFFIX);
        async move {
            let program_path = path.bin.project_managerexe.as_path();
            ide_ci::fs::allow_owner_execute(program_path)?;
            let output = Command::new(program_path).arg("--version").output_ok().await?;
            let string = String::from_utf8(output.stdout)?;
            let version = find_in_text(&string)?;
            Ok(Self { path, versions: Versions::new(version) })
        }
        .boxed()
    }
}

// impl Artifact {
//     pub fn project_manager_cmd(&self) -> crate::programs::project_manager::Command {
//         Command::new(&self.path.bin.project_managerexe).into()
//     }
// }

#[derive(Clone, Debug)]
pub struct ProjectManager;

#[async_trait]
impl IsTarget for ProjectManager {
    type BuildInput = BuildInput;
    type Artifact = Artifact;

    fn artifact_name(&self) -> &str {
        // Version is not part of the name intentionally. We want to refer to PM bundles as
        // artifacts without knowing their version.
        static NAME: SyncLazy<String> = SyncLazy::new(|| format!("project-manager-{}", TARGET_OS));
        &*NAME
    }

    fn build(
        &self,
        input: Self::BuildInput,
        output_path: impl AsRef<Path> + Send + Sync + 'static,
    ) -> BoxFuture<'static, Result<Self::Artifact>> {
        async move {
            let paths =
                crate::paths::Paths::new_versions(&input.repo_root, input.versions.clone())?;
            let context = crate::engine::context::RunContext {
                operation: crate::engine::Operation::Build(BuildOperation {}),
                goodies: GoodieDatabase::new()?,
                config: BuildConfiguration {
                    clean_repo: false,
                    build_project_manager_bundle: true,
                    ..crate::engine::NIGHTLY
                },
                octocrab: input.octocrab.clone(),
                paths,
            };
            let artifacts = context.build().await?;
            let project_manager =
                artifacts.bundles.project_manager.context("Missing project manager bundle!")?;
            ide_ci::fs::mirror_directory(&project_manager.dir, &output_path).await?;
            Artifact::from_existing(output_path.as_ref()).await
        }
        .boxed()
    }

    fn find_asset(&self, assets: Vec<Asset>) -> Result<Asset> {
        assets
            .into_iter()
            .find(|asset| {
                let name = &asset.name;
                matches_platform(name) && is_archive_name(name) && name.contains("project-manager")
            })
            .context("Failed to find release asset with project manager bundle.")
    }
}

pub fn matches_platform(name: &str) -> bool {
    // Sample name: "project-manager-bundle-2022.1.1-nightly.2022-04-16-linux-amd64.tar.gz"
    name.contains(TARGET_OS.as_str()) && name.contains(pretty_print_arch(TARGET_ARCH))
    // TODO workaround for macOS and M1 (they should be allowed to use amd64 artifacts)
}
