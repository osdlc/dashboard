//! Tokens and passwords for modules that need one. They never go in the
//! config file: a module names an environment variable (`token_env`) or a
//! command that prints the secret (`token_command`, e.g. a password manager CLI).

use anyhow::{Context, Result, bail};
use schemars::JsonSchema;
use serde::Deserialize;

/// Where a module gets its API token (or password).
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct Secret {
    /// Environment variable holding the token.
    pub token_env: Option<String>,
    /// Command that prints the token, e.g. `["op", "read", "op://dev/linear/token"]`.
    pub token_command: Option<Vec<String>>,
}

impl Secret {
    /// Reads the token: the configured command, else the variable (or `default_env`).
    pub async fn token(&self, default_env: &str) -> Result<String> {
        match self.token_opt(default_env).await? {
            Some(token) => Ok(token),
            None => {
                let var = self.token_env.as_deref().unwrap_or(default_env);
                bail!(
                    "no API token: set ${var} before starting the dashboard, or set token_env / token_command in the config"
                )
            }
        }
    }

    /// Like `token`, for services where the secret is optional (a local
    /// service with no password): None when the variable is unset or empty
    /// and there is no command. A failing command is still an error.
    pub async fn token_opt(&self, default_env: &str) -> Result<Option<String>> {
        if let Some(argv) = &self.token_command {
            let (program, args) = argv.split_first().context("token_command is empty")?;
            let out = tokio::process::Command::new(program)
                .args(args)
                .stdin(std::process::Stdio::null())
                .output()
                .await
                .with_context(|| format!("running token_command {program:?}"))?;
            if !out.status.success() {
                bail!(
                    "token_command {program:?} failed: {}",
                    String::from_utf8_lossy(&out.stderr).trim()
                );
            }
            let token = String::from_utf8_lossy(&out.stdout).trim().to_string();
            if token.is_empty() {
                bail!("token_command {program:?} printed nothing");
            }
            return Ok(Some(token));
        }
        let var = self.token_env.as_deref().unwrap_or(default_env);
        Ok(std::env::var(var)
            .ok()
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn missing_token_names_the_variable() {
        let s = Secret {
            token_env: Some("DASHBOARD_TEST_NO_SUCH_SECRET".into()),
            token_command: None,
        };
        assert_eq!(s.token_opt("X").await.unwrap(), None);
        let err = s.token("X").await.unwrap_err().to_string();
        assert!(err.contains("$DASHBOARD_TEST_NO_SUCH_SECRET"), "{err}");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn command_output_is_trimmed() {
        let s = Secret {
            token_env: None,
            token_command: Some(vec!["echo".into(), " abc ".into()]),
        };
        assert_eq!(s.token("X").await.unwrap(), "abc");
    }
}
