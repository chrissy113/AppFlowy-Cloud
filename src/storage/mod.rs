use anyhow::{Error, Result};
use async_trait::async_trait;
use bytes::Bytes;
use futures::StreamExt;
use octocrab::Octocrab;
use serde::{Deserialize, Serialize};
use std::env;
use tokio::io::AsyncReadExt;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub enum StorageType {
    S3,
    GitHub,
    Local,
}

impl Default for StorageType {
    fn default() -> Self {
        StorageType::GitHub
    }
}

#[async_trait]
pub trait Storage: Send + Sync {
    async fn upload_file(&self, file_path: &str, content: Vec<u8>) -> Result<String>;
    async fn download_file(&self, file_path: &str) -> Result<Vec<u8>>;
    async fn delete_file(&self, file_path: &str) -> Result<()>;
}

pub struct GitHubStorage {
    client: Octocrab,
    repo_owner: String,
    repo_name: String,
}

impl GitHubStorage {
    pub fn new() -> Result<Self> {
        let github_token = env::var("GITHUB_TOKEN").expect("GITHUB_TOKEN must be set");
        let github_repo = env::var("GITHUB_REPO").expect("GITHUB_REPO must be set");
        let repo_parts: Vec<&str> = github_repo.split('/').collect();
        
        if repo_parts.len() != 2 {
            return Err(Error::msg("GITHUB_REPO must be in format owner/repo"));
        }

        let client = Octocrab::builder()
            .personal_token(github_token)
            .build()?;

        Ok(Self {
            client,
            repo_owner: repo_parts[0].to_string(),
            repo_name: repo_parts[1].to_string(),
        })
    }
}

#[async_trait]
impl Storage for GitHubStorage {
    async fn upload_file(&self, file_path: &str, content: Vec<u8>) -> Result<String> {
        // Create a new release with the file path as tag
        let tag_name = format!("file_{}", file_path.replace('/', "_"));
        
        // Create release
        let release = self.client
            .repos(&self.repo_owner, &self.repo_name)
            .releases()
            .create(&tag_name)
            .name(&tag_name)
            .send()
            .await?;

        // Upload the file to the release
        let asset = self.client
            .repos(&self.repo_owner, &self.repo_name)
            .releases()
            .upload_asset(
                &release,
                "application/octet-stream",
                file_path.split('/').last().unwrap_or(file_path),
                &content,
            )
            .await?;

        Ok(asset.browser_download_url.unwrap_or_default())
    }

    async fn download_file(&self, file_path: &str) -> Result<Vec<u8>> {
        let tag_name = format!("file_{}", file_path.replace('/', "_"));
        
        // Get release by tag
        let release = self.client
            .repos(&self.repo_owner, &self.repo_name)
            .releases()
            .get_by_tag(&tag_name)
            .await?;

        // Get the first asset
        let asset = release.assets.first()
            .ok_or_else(|| Error::msg("No assets found in release"))?;

        // Download the asset
        let response = reqwest::get(&asset.browser_download_url)
            .await?;
        
        Ok(response.bytes().await?.to_vec())
    }

    async fn delete_file(&self, file_path: &str) -> Result<()> {
        let tag_name = format!("file_{}", file_path.replace('/', "_"));
        
        // Delete release by tag
        self.client
            .repos(&self.repo_owner, &self.repo_name)
            .releases()
            .delete_by_tag(&tag_name)
            .await?;

        Ok(())
    }
}

pub struct StorageService {
    storage: Box<dyn Storage>,
}

impl StorageService {
    pub fn new() -> Result<Self> {
        let storage_type = env::var("STORAGE_TYPE")
            .unwrap_or_else(|_| "github".to_string());

        let storage: Box<dyn Storage> = match storage_type.as_str() {
            "github" => Box::new(GitHubStorage::new()?),
            _ => return Err(Error::msg("Unsupported storage type")),
        };

        Ok(Self { storage })
    }

    pub async fn upload_file(&self, file_path: &str, content: Vec<u8>) -> Result<String> {
        self.storage.upload_file(file_path, content).await
    }

    pub async fn download_file(&self, file_path: &str) -> Result<Vec<u8>> {
        self.storage.download_file(file_path).await
    }

    pub async fn delete_file(&self, file_path: &str) -> Result<()> {
        self.storage.delete_file(file_path).await
    }
}
