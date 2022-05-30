use crate::prelude::*;

use octocrab::models::ArtifactId;

use crate::cache::Cache;
use crate::cache::Storable;
use crate::models::config::RepoContext;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Key {
    pub repository:  RepoContext,
    pub artifact_id: ArtifactId,
}

#[derive(Clone, Debug)]
pub struct ExtractedArtifact {
    pub key:    Key,
    pub client: Octocrab,
}

impl Borrow<Key> for ExtractedArtifact {
    fn borrow(&self) -> &Key {
        &self.key
    }
}

impl Storable for ExtractedArtifact {
    type Metadata = ();
    type Output = PathBuf;
    type Key = Key;

    fn generate(
        &self,
        _cache: Cache,
        store: PathBuf,
    ) -> BoxFuture<'static, Result<Self::Metadata>> {
        let this = self.clone();
        async move {
            let ExtractedArtifact { client, key } = this;
            let Key { artifact_id, repository } = key;
            repository.download_and_unpack_artifact(&client, artifact_id, &store).await?;
            Ok(())
        }
        .boxed()
    }

    fn adapt(
        &self,
        cache: PathBuf,
        _metadata: Self::Metadata,
    ) -> BoxFuture<'static, Result<Self::Output>> {
        ready(Result::Ok(cache)).boxed()
    }
}
