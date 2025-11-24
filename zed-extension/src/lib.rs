use zed_extension_api::{self as zed, Result};

struct SquirrelExtension;

impl SquirrelExtension {
    const SERVER_BINARY_NAME: &'static str = "squirrel-lsp";
}

impl zed::Extension for SquirrelExtension {
    fn new() -> Self {
        Self
    }

    fn language_server_command(
        &mut self,
        _language_server_id: &zed::LanguageServerId,
        worktree: &zed::Worktree,
    ) -> Result<zed::Command> {
        let path = worktree
            .which(Self::SERVER_BINARY_NAME)
            .ok_or_else(|| {
                format!(
                    "Could not find '{}' in PATH. Please install squirrel-lsp: \
                    cargo install --git https://github.com/mnshdw/squirrel-lsp",
                    Self::SERVER_BINARY_NAME
                )
            })?;

        Ok(zed::Command {
            command: path,
            args: vec![],
            env: worktree.shell_env(),
        })
    }
}

zed::register_extension!(SquirrelExtension);
