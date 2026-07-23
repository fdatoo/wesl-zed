use std::{
    fs,
    path::{Path, PathBuf},
};

use zed::settings::LspSettings;
use zed_extension_api::{self as zed, LanguageServerId, Result};

const SERVER_NAME: &str = "wesl-lsp";
const RELEASE_REPOSITORY: &str = "fdatoo/wesl-lsp";

struct WeslExtension {
    cached_binary_path: Option<String>,
}

impl WeslExtension {
    fn downloaded_binary_path(&mut self, language_server_id: &LanguageServerId) -> Result<String> {
        if let Some(path) = &self.cached_binary_path
            && fs::metadata(path).is_ok_and(|metadata| metadata.is_file())
        {
            return Ok(path.clone());
        }

        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );
        let release = zed::latest_github_release(
            RELEASE_REPOSITORY,
            zed::GithubReleaseOptions {
                require_assets: true,
                pre_release: false,
            },
        )?;
        let (platform, architecture) = zed::current_platform();
        let (target, extension, archive_type) = match (platform, architecture) {
            (zed::Os::Mac, zed::Architecture::Aarch64) => (
                "aarch64-apple-darwin",
                "tar.gz",
                zed::DownloadedFileType::GzipTar,
            ),
            (zed::Os::Mac, _) => (
                "x86_64-apple-darwin",
                "tar.gz",
                zed::DownloadedFileType::GzipTar,
            ),
            (zed::Os::Linux, zed::Architecture::Aarch64) => (
                "aarch64-unknown-linux-gnu",
                "tar.gz",
                zed::DownloadedFileType::GzipTar,
            ),
            (zed::Os::Linux, _) => (
                "x86_64-unknown-linux-gnu",
                "tar.gz",
                zed::DownloadedFileType::GzipTar,
            ),
            (zed::Os::Windows, zed::Architecture::Aarch64) => {
                return Err("wesl-lsp does not publish a Windows ARM64 binary".to_owned());
            }
            (zed::Os::Windows, _) => (
                "x86_64-pc-windows-msvc",
                "zip",
                zed::DownloadedFileType::Zip,
            ),
        };
        let asset_name = format!("wesl-lsp-{}-{target}.{extension}", release.version);
        let asset = release
            .assets
            .iter()
            .find(|asset| asset.name == asset_name)
            .ok_or_else(|| format!("release {} has no {asset_name} asset", release.version))?;
        let version_dir = format!("wesl-lsp-{}", release.version);
        let binary_name = if platform == zed::Os::Windows {
            "wesl-lsp.exe"
        } else {
            "wesl-lsp"
        };
        let binary_path = Path::new(&version_dir).join(binary_name);
        if !binary_path.is_file() {
            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );
            fs::create_dir_all(&version_dir)
                .map_err(|error| format!("failed to create {version_dir}: {error}"))?;
            zed::download_file(&asset.download_url, &version_dir, archive_type)
                .map_err(|error| format!("failed to download {asset_name}: {error}"))?;
            let extracted = find_binary(Path::new(&version_dir), binary_name)
                .ok_or_else(|| format!("{asset_name} did not contain {binary_name}"))?;
            if extracted != binary_path {
                fs::rename(&extracted, &binary_path).map_err(|error| {
                    format!(
                        "failed to move {} to {}: {error}",
                        extracted.display(),
                        binary_path.display()
                    )
                })?;
            }
            zed::make_file_executable(binary_path.to_string_lossy().as_ref())?;
            remove_stale_versions(&version_dir)?;
        }
        let binary_path = binary_path.to_string_lossy().into_owned();
        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

impl zed::Extension for WeslExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let settings = LspSettings::for_worktree(SERVER_NAME, worktree).unwrap_or_default();
        let args = settings
            .binary
            .as_ref()
            .and_then(|binary| binary.arguments.clone())
            .unwrap_or_default();
        if let Some(path) = settings.binary.and_then(|binary| binary.path) {
            return Ok(zed::Command {
                command: path,
                args,
                env: worktree.shell_env(),
            });
        }
        if let Some(path) = worktree.which(SERVER_NAME) {
            return Ok(zed::Command {
                command: path,
                args,
                env: worktree.shell_env(),
            });
        }
        Ok(zed::Command {
            command: self.downloaded_binary_path(language_server_id)?,
            args,
            env: worktree.shell_env(),
        })
    }

    fn language_server_initialization_options(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(SERVER_NAME, worktree)
            .ok()
            .and_then(|settings| settings.initialization_options))
    }

    fn language_server_workspace_configuration(
        &mut self,
        _language_server_id: &LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<Option<zed::serde_json::Value>> {
        Ok(LspSettings::for_worktree(SERVER_NAME, worktree)
            .ok()
            .and_then(|settings| settings.settings))
    }
}

fn find_binary(root: &Path, binary_name: &str) -> Option<PathBuf> {
    let direct = root.join(binary_name);
    if direct.is_file() {
        return Some(direct);
    }
    fs::read_dir(root)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path().join(binary_name))
        .find(|candidate| candidate.is_file())
}

fn remove_stale_versions(current: &str) -> Result<()> {
    for entry in fs::read_dir(".").map_err(|error| error.to_string())? {
        let entry = entry.map_err(|error| error.to_string())?;
        let name = entry.file_name();
        let Some(name) = name.to_str() else {
            continue;
        };
        if name != current && name.starts_with("wesl-lsp-") && entry.path().is_dir() {
            fs::remove_dir_all(entry.path()).map_err(|error| error.to_string())?;
        }
    }
    Ok(())
}

zed::register_extension!(WeslExtension);
