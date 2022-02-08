use crate::prelude::*;

use anyhow::Context;
use bytes::BytesMut;
use reqwest::Body;
use reqwest::Response;
use reqwest::StatusCode;
use serde::de::DeserializeOwned;
use tokio::io::AsyncReadExt;

use crate::actions::artifacts::models::ArtifactResponse;
use crate::actions::artifacts::models::CreateArtifactRequest;
use crate::actions::artifacts::models::CreateArtifactResponse;
use crate::actions::artifacts::models::ListArtifactsResponse;
use crate::actions::artifacts::models::PatchArtifactSize;
use crate::actions::artifacts::models::PatchArtifactSizeResponse;
use crate::actions::artifacts::models::QueryArtifactResponse;
use crate::reqwest::ContentRange;

pub mod endpoints {
    use super::*;

    /// Creates a file container for the new artifact in the remote blob storage/file service.
    ///
    /// Returns the response from the Artifact Service if the file container was successfully
    /// create.
    #[context("Failed to create a file container for the new  artifact `{}`.", artifact_name.as_ref())]
    pub async fn create_container(
        json_client: &reqwest::Client,
        artifact_url: Url,
        artifact_name: impl AsRef<str>,
    ) -> Result<CreateArtifactResponse> {
        let body = CreateArtifactRequest::new(artifact_name.as_ref(), None);
        //
        // dbg!(&self.json_client);
        // dbg!(serde_json::to_string(&body)?);
        let request = json_client.post(artifact_url).json(&body).build()?;

        // dbg!(&request);
        // TODO retry
        let response = json_client.execute(request).await?;
        // dbg!(&response);
        // let status = response.status();
        check_response_json(response, |status, err| match status {
            StatusCode::FORBIDDEN => err.context(
                "Artifact storage quota has been hit. Unable to upload any new artifacts.",
            ),
            StatusCode::BAD_REQUEST => err.context(format!(
                "Server rejected the request. Is the artifact name {} valid?",
                artifact_name.as_ref()
            )),
            _ => err,
        })
        .await
    }

    pub async fn upload_file_chunk(
        client: &reqwest::Client,
        upload_url: Url,
        body: impl Into<Body>,
        range: ContentRange,
        remote_path: impl AsRef<Path>,
    ) -> Result<usize> {
        use path_slash::PathExt;
        let body = body.into();
        let response = client
            .put(upload_url)
            .query(&[("itemPath", remote_path.as_ref().to_slash_lossy())])
            .header(reqwest::header::CONTENT_LENGTH, range.len())
            .header(reqwest::header::CONTENT_RANGE, &range)
            .body(body)
            .send()
            .await?;

        check_response(response, |_, e| e).await?;
        Ok(range.len())
    }

    #[context("Failed to list artifacts for the current run.")]
    pub async fn list_artifacts(
        json_client: &reqwest::Client,
        artifact_url: Url,
    ) -> Result<Vec<ArtifactResponse>> {
        Ok(json_client.get(artifact_url).send().await?.json::<ListArtifactsResponse>().await?.value)
    }

    #[context("Getting container items of artifact {}.", artifact_name.as_ref())]
    pub async fn get_container_items(
        json_client: &reqwest::Client,
        container_url: Url,
        artifact_name: impl AsRef<str>,
    ) -> Result<QueryArtifactResponse> {
        let body = json_client
            .get(container_url)
            .query(&item_path_query(&artifact_name.as_ref()))
            .send()
            .await?
            .json::<serde_json::Value>()
            .await?;
        println!("{}", serde_json::to_string_pretty(&body)?);
        serde_json::from_value(body).anyhow_err()
    }

    #[context("Failed to finalize upload of the artifact `{}`.", artifact_name.as_ref())]
    pub async fn patch_artifact_size(
        json_client: &reqwest::Client,
        artifact_url: Url,
        artifact_name: impl AsRef<str>,
        size: usize,
    ) -> Result<PatchArtifactSizeResponse> {
        println!("Patching the artifact `{}` size.", artifact_name.as_ref());
        let artifact_url = artifact_url.clone();

        let patch_request = json_client
            .patch(artifact_url.clone())
            .query(&[("artifactName", artifact_name.as_ref())]) // OsStr can be passed here, fails runtime
            .json(&PatchArtifactSize { size });

        // TODO retry
        let response = patch_request.send().await?;
        Ok(response.json().await?)
    }
}


#[context("Failed to upload the file '{}' to path '{}'.", local_path.as_ref().display(), remote_path.as_ref().display())]
pub async fn upload_file(
    client: &reqwest::Client,
    chunk_size: usize,
    upload_url: Url,
    local_path: impl AsRef<Path>,
    remote_path: impl AsRef<Path>,
) -> Result<usize> {
    let file = tokio::fs::File::open(local_path.as_ref()).await?;
    // TODO [mwu] note that metadata can lie about file size, e.g. named pipes on Linux
    let len = file.metadata().await?.len() as usize;
    println!("Will upload file {} of size {}", local_path.as_ref().display(), len);
    if len < chunk_size && len > 0 {
        let range = ContentRange::whole(len as usize);
        endpoints::upload_file_chunk(client, upload_url.clone(), file, range, &remote_path).await
    } else {
        let mut chunks = stream_file_in_chunks(file, chunk_size).boxed();
        let mut current_position = 0;
        loop {
            let chunk = match chunks.try_next().await? {
                Some(chunk) => chunk,
                None => break,
            };

            let read_bytes = chunk.len();
            let range = ContentRange {
                range: current_position..=current_position + read_bytes.saturating_sub(1),
                total: Some(len),
            };
            endpoints::upload_file_chunk(client, upload_url.clone(), chunk, range, &remote_path)
                .await?;
            current_position += read_bytes;
        }
        Ok(current_position)
    }
}

pub async fn check_response_json<T: DeserializeOwned>(
    response: Response,
    additional_context: impl FnOnce(StatusCode, anyhow::Error) -> anyhow::Error,
) -> Result<T> {
    let data = check_response(response, additional_context).await?;
    serde_json::from_slice(data.as_ref()).context(anyhow!(
        "Failed to deserialize response body as {}. Body was: {:?}",
        std::any::type_name::<T>(),
        data,
    ))
}
pub async fn check_response(
    response: Response,
    additional_context: impl FnOnce(StatusCode, anyhow::Error) -> anyhow::Error,
) -> Result<Bytes> {
    // dbg!(&response);
    let status = response.status();
    if !status.is_success() {
        let mut err = anyhow!("Server replied with status {}.", status);

        let body = response
            .bytes()
            .await
            .map_err(|e| anyhow!("Also failed to obtain the response body: {}", e))?;

        if let Ok(body_text) = std::str::from_utf8(body.as_ref()) {
            err = err.context(format!("Error response body was: {}", body_text));
        }

        let err = additional_context(status, err);
        Err(err)
    } else {
        response.bytes().await.context("Failed to read the response body.")
    }
}

pub fn stream_file_in_chunks(
    file: tokio::fs::File,
    chunk_size: usize,
) -> impl futures::Stream<Item = Result<Bytes>> + Send {
    futures::stream::try_unfold(file, async move |mut file| {
        let mut buffer = BytesMut::with_capacity(chunk_size);
        while file.read_buf(&mut buffer).await? > 0 && buffer.len() < chunk_size {}
        if buffer.is_empty() {
            Ok::<_, anyhow::Error>(None)
        } else {
            Ok(Some((buffer.freeze(), file)))
        }
    })
}

pub fn item_path_query(artifact_name: impl Serialize) -> impl Serialize {
    [("itemPath", artifact_name)]
}