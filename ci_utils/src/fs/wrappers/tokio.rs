use crate::prelude::*;

use tokio::fs::File;


#[context("Failed to obtain metadata for file: {}", path.as_ref().display())]
pub async fn metadata<P: AsRef<Path>>(path: P) -> Result<std::fs::Metadata> {
    tokio::fs::metadata(&path).await.anyhow_err()
}

#[context("Failed to open path for reading: {}", path.as_ref().display())]
pub async fn open(path: impl AsRef<Path>) -> Result<File> {
    File::open(&path).await.anyhow_err()
}

#[context("Failed to open path for writing: {}", path.as_ref().display())]
pub async fn create(path: impl AsRef<Path>) -> Result<File> {
    File::create(&path).await.anyhow_err()
}

#[context("Failed to create missing directories no path: {}", path.as_ref().display())]
pub async fn create_dir_all(path: impl AsRef<Path>) -> Result {
    tokio::fs::create_dir_all(&path).await.anyhow_err()
}

#[context("Failed to read the directory: {}", path.as_ref().display())]
pub async fn read_dir(path: impl AsRef<Path>) -> Result<tokio::fs::ReadDir> {
    tokio::fs::read_dir(&path).await.anyhow_err()
}
