use crate::prelude::*;

use crate::ide::web::IdeDesktop;
use crate::paths::generated::RepoRoot;
use crate::project::wasm;
use crate::project::IsTarget;
use crate::project::PlainArtifact;
use crate::BoxFuture;

pub type Artifact = PlainArtifact<Gui>;

#[derive(derivative::Derivative)]
#[derivative(Debug)]
pub struct GuiInputs {
    pub repo_root:  RepoRoot,
    #[derivative(Debug = "ignore")]
    pub wasm:       BoxFuture<'static, Result<wasm::Artifacts>>,
    #[derivative(Debug = "ignore")]
    pub build_info: BoxFuture<'static, Result<BuildInfo>>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Gui;

impl Gui {
    pub async fn watch(&self, input: GuiInputs) -> Result {
        let ide = IdeDesktop::new(&input.repo_root.app.ide_desktop);
        ide.watch(input.wasm, &input.build_info.await?).await?;
        Ok(())
    }
}

#[async_trait]
impl IsTarget for Gui {
    type BuildInput = GuiInputs;
    type Output = Artifact;

    fn artifact_name(&self) -> &str {
        "gui"
    }


    fn build(
        &self,
        input: Self::BuildInput,
        output_path: impl AsRef<Path> + Send + Sync + 'static,
    ) -> BoxFuture<'static, Result<Self::Output>> {
        async move {
            let ide = IdeDesktop::new(&input.repo_root.app.ide_desktop);
            ide.build(input.wasm, &input.build_info.await?, &output_path).await?;
            Ok(Artifact::new(output_path.as_ref()))
        }
        .boxed()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildInfo {
    pub commit:         String,
    pub version:        Version,
    pub engine_version: Version,
    pub name:           String,
}
