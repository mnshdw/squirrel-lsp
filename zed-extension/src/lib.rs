use std::fs;
use zed_extension_api::{self as zed, Result, settings::LspSettings};

const SERVER_BINARY_NAME: &str = "squirrel-lsp";
const GITHUB_REPO: &str = "mnshdw/squirrel-lsp";

struct SquirrelExtension {
    cached_binary_path: Option<String>,
}

impl SquirrelExtension {
    fn language_server_binary_path(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<String> {
        // Check if user configured a custom binary path
        if let Some(path) = worktree.which(SERVER_BINARY_NAME) {
            return Ok(path);
        }

        // Check LSP settings for custom binary path
        if let Some(lsp_settings) = LspSettings::for_worktree(SERVER_BINARY_NAME, worktree).ok() {
            if let Some(binary) = lsp_settings.binary {
                if let Some(path) = binary.path {
                    return Ok(path);
                }
            }
        }

        // Use cached path if available
        if let Some(path) = &self.cached_binary_path {
            if fs::metadata(path).map(|m| m.is_file()).unwrap_or(false) {
                return Ok(path.clone());
            }
        }

        // Download from GitHub releases
        zed::set_language_server_installation_status(
            language_server_id,
            &zed::LanguageServerInstallationStatus::CheckingForUpdate,
        );

        let release = zed::github_release_by_tag_name(GITHUB_REPO, "latest").or_else(|_| {
            zed::latest_github_release(
                GITHUB_REPO,
                zed::GithubReleaseOptions {
                    require_assets: true,
                    pre_release: false,
                },
            )
        })?;

        let (platform, arch) = zed::current_platform();
        let asset_name = get_asset_name(platform, arch)?;

        let asset = release
            .assets
            .iter()
            .find(|a| a.name == asset_name)
            .ok_or_else(|| format!("No release asset found for {asset_name}"))?;

        let version_dir = format!("squirrel-lsp-{}", release.version);
        let binary_name = if matches!(platform, zed::Os::Windows) {
            format!("{SERVER_BINARY_NAME}.exe")
        } else {
            SERVER_BINARY_NAME.to_string()
        };
        let binary_path = format!("{version_dir}/{binary_name}");

        if !fs::metadata(&binary_path)
            .map(|m| m.is_file())
            .unwrap_or(false)
        {
            fs::create_dir_all(&version_dir)
                .map_err(|e| format!("Failed to create directory {version_dir}: {e}"))?;

            zed::set_language_server_installation_status(
                language_server_id,
                &zed::LanguageServerInstallationStatus::Downloading,
            );

            zed::download_file(
                &asset.download_url,
                &binary_path,
                zed::DownloadedFileType::Uncompressed,
            )
            .map_err(|e| format!("Failed to download {}: {e}", asset.name))?;

            zed::make_file_executable(&binary_path)?;
        }

        self.cached_binary_path = Some(binary_path.clone());
        Ok(binary_path)
    }
}

fn get_asset_name(platform: zed::Os, arch: zed::Architecture) -> Result<String> {
    let os = match platform {
        zed::Os::Mac => "macos",
        zed::Os::Linux => "linux",
        zed::Os::Windows => "windows",
    };

    let arch = match arch {
        zed::Architecture::Aarch64 => "aarch64",
        zed::Architecture::X8664 => "x86_64",
        _ => return Err(format!("Unsupported architecture: {arch:?}")),
    };

    let ext = if matches!(platform, zed::Os::Windows) {
        ".exe"
    } else {
        ""
    };

    Ok(format!("squirrel-lsp-{os}-{arch}{ext}"))
}

impl zed::Extension for SquirrelExtension {
    fn new() -> Self {
        Self {
            cached_binary_path: None,
        }
    }

    fn language_server_command(
        &mut self,
        language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let binary_path = self.language_server_binary_path(language_server_id, worktree)?;

        Ok(zed::Command {
            command: binary_path,
            args: vec![],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(SquirrelExtension);
