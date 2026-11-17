//! Where this run points, and how it listens for the sign-in callback.

use url::Url;

use crate::error::Error;

/// The two deployments a build talks to. A debug build starts on ddev, a
/// release build on production, so `cargo run` needs no flags and a shipped
/// binary needs none either.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Environment {
    Ddev,
    Prod,
}

impl Environment {
    pub fn label(self) -> &'static str {
        match self {
            Self::Ddev => "ddev",
            Self::Prod => "prod",
        }
    }

    pub fn base_url(self) -> &'static str {
        match self {
            Self::Ddev => "https://projectsovereign.ddev.site",
            Self::Prod => "https://lynxby.com",
        }
    }

    pub fn for_profile() -> Self {
        if cfg!(debug_assertions) {
            Self::Ddev
        } else {
            Self::Prod
        }
    }

    fn parse(value: &str) -> Result<Self, Error> {
        match value.trim().to_ascii_lowercase().as_str() {
            "ddev" | "local" | "dev" => Ok(Self::Ddev),
            "prod" | "production" => Ok(Self::Prod),
            other => Err(Error::Config(format!(
                "Unknown environment '{other}'. Use ddev or prod."
            ))),
        }
    }
}

/// A loopback port that is unlikely to clash and easy to allow in a firewall.
pub const DEFAULT_REDIRECT_PORT: u16 = 47833;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Config {
    pub environment: Environment,
    pub base_url: String,
    pub redirect_port: u16,
    /// Set when the deployment hands out client ids itself instead of letting
    /// a client register.
    pub client_id: Option<String>,
}

impl Config {
    pub fn from_process() -> Result<Self, Error> {
        let args: Vec<String> = std::env::args().skip(1).collect();

        Self::resolve(
            &args,
            &|key| std::env::var(key).ok(),
            Environment::for_profile(),
        )
    }

    /// Precedence: flag, then environment variable, then the profile default.
    pub fn resolve(
        args: &[String],
        getenv: &dyn Fn(&str) -> Option<String>,
        default_environment: Environment,
    ) -> Result<Self, Error> {
        let mut environment = getenv("LYNXBY_ENV")
            .map(|value| Environment::parse(&value))
            .transpose()?
            .unwrap_or(default_environment);
        let mut base_url: Option<String> = getenv("LYNXBY_BASE_URL");
        let mut port: Option<u16> = None;

        let mut index = 0;
        while index < args.len() {
            let argument = args[index].as_str();
            let (flag, inline) = match argument.split_once('=') {
                Some((flag, value)) => (flag, Some(value.to_string())),
                None => (argument, None),
            };

            let mut value = |name: &str| -> Result<String, Error> {
                if let Some(inline) = inline.clone() {
                    return Ok(inline);
                }

                index += 1;
                args.get(index)
                    .cloned()
                    .ok_or_else(|| Error::Config(format!("{name} needs a value.")))
            };

            match flag {
                "--env" => environment = Environment::parse(&value("--env")?)?,
                "--base-url" => base_url = Some(value("--base-url")?),
                "--port" => {
                    let raw = value("--port")?;
                    port = Some(
                        raw.parse()
                            .map_err(|_| Error::Config(format!("'{raw}' is not a port.")))?,
                    );
                }
                "--help" | "-h" => return Err(Error::Config(usage())),
                other => {
                    return Err(Error::Config(format!(
                        "Unknown option '{other}'.\n\n{}",
                        usage()
                    )))
                }
            }

            index += 1;
        }

        let redirect_port = match port {
            Some(port) => port,
            None => match getenv("LYNXBY_REDIRECT_PORT") {
                Some(raw) => raw
                    .parse()
                    .map_err(|_| Error::Config(format!("'{raw}' is not a port.")))?,
                None => DEFAULT_REDIRECT_PORT,
            },
        };

        let base_url =
            trim_base_url(base_url.unwrap_or_else(|| environment.base_url().to_string()))?;

        Ok(Self {
            environment,
            base_url,
            redirect_port,
            client_id: getenv("LYNXBY_CLIENT_ID").filter(|value| !value.trim().is_empty()),
        })
    }

    pub fn redirect_uri(&self) -> String {
        format!("http://127.0.0.1:{}/callback", self.redirect_port)
    }

    pub fn host(&self) -> String {
        Url::parse(&self.base_url)
            .ok()
            .and_then(|url| url.host_str().map(str::to_string))
            .unwrap_or_else(|| self.base_url.clone())
    }
}

fn trim_base_url(raw: String) -> Result<String, Error> {
    let url =
        Url::parse(raw.trim()).map_err(|_| Error::Config(format!("'{raw}' is not a URL.")))?;

    let local = matches!(url.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));

    if url.scheme() != "https" && !(url.scheme() == "http" && local) {
        return Err(Error::Config(
            "The base URL must be https (http is allowed on localhost only).".to_string(),
        ));
    }

    if url.query().is_some() || url.fragment().is_some() {
        return Err(Error::Config(
            "The base URL cannot carry a query or a fragment.".to_string(),
        ));
    }

    Ok(format!(
        "{}{}",
        url.origin().ascii_serialization(),
        url.path().trim_end_matches('/')
    ))
}

fn usage() -> String {
    "Lynxby Desktop\n\
     \n\
     Usage: lynxby-desktop [--env ddev|prod] [--base-url URL] [--port PORT]\n\
     \n\
     --env       deployment to talk to (default: ddev in a debug build, prod otherwise)\n\
     --base-url  override the deployment URL\n\
     --port      loopback port for the sign-in callback\n\
     \n\
     Environment: LYNXBY_ENV, LYNXBY_BASE_URL, LYNXBY_REDIRECT_PORT, LYNXBY_CLIENT_ID"
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn no_env(_: &str) -> Option<String> {
        None
    }

    fn args(values: &[&str]) -> Vec<String> {
        values.iter().map(|value| value.to_string()).collect()
    }

    #[test]
    fn a_debug_build_points_at_ddev() {
        let config = Config::resolve(&[], &no_env, Environment::Ddev).unwrap();

        assert_eq!(config.base_url, "https://projectsovereign.ddev.site");
        assert_eq!(config.environment, Environment::Ddev);
        assert_eq!(config.redirect_uri(), "http://127.0.0.1:47833/callback");
    }

    #[test]
    fn a_release_build_points_at_production() {
        let config = Config::resolve(&[], &no_env, Environment::Prod).unwrap();

        assert_eq!(config.base_url, "https://lynxby.com");
    }

    #[test]
    fn the_flag_beats_the_environment() {
        let config = Config::resolve(
            &args(&["--env", "prod"]),
            &|key| (key == "LYNXBY_ENV").then(|| "ddev".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.environment, Environment::Prod);
        assert_eq!(config.base_url, "https://lynxby.com");
    }

    #[test]
    fn the_environment_beats_the_profile_default() {
        let config = Config::resolve(
            &[],
            &|key| (key == "LYNXBY_ENV").then(|| "prod".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.environment, Environment::Prod);
    }

    #[test]
    fn a_base_url_override_wins_and_loses_its_trailing_slash() {
        let config = Config::resolve(
            &args(&["--base-url", "https://staging.example.test/"]),
            &no_env,
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.base_url, "https://staging.example.test");
    }

    #[test]
    fn the_environment_variable_can_carry_the_base_url() {
        let config = Config::resolve(
            &[],
            &|key| (key == "LYNXBY_BASE_URL").then(|| "https://tenant.example.test".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.base_url, "https://tenant.example.test");
    }

    #[test]
    fn accepts_both_flag_shapes() {
        let spaced =
            Config::resolve(&args(&["--env", "prod"]), &no_env, Environment::Ddev).unwrap();
        let joined = Config::resolve(&args(&["--env=prod"]), &no_env, Environment::Ddev).unwrap();

        assert_eq!(spaced, joined);
    }

    #[test]
    fn reads_a_port_from_flag_and_environment() {
        let flagged =
            Config::resolve(&args(&["--port", "51000"]), &no_env, Environment::Ddev).unwrap();
        let from_env = Config::resolve(
            &[],
            &|key| (key == "LYNXBY_REDIRECT_PORT").then(|| "52000".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(flagged.redirect_port, 51000);
        assert_eq!(flagged.redirect_uri(), "http://127.0.0.1:51000/callback");
        assert_eq!(from_env.redirect_port, 52000);
    }

    #[test]
    fn rejects_a_plain_http_deployment() {
        let error = Config::resolve(
            &args(&["--base-url", "http://lynxby.example.test"]),
            &no_env,
            Environment::Ddev,
        )
        .unwrap_err();

        assert!(error.to_string().contains("must be https"));
    }

    #[test]
    fn allows_plain_http_on_loopback_for_development() {
        let config = Config::resolve(
            &args(&["--base-url", "http://127.0.0.1:8000"]),
            &no_env,
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.base_url, "http://127.0.0.1:8000");
    }

    #[test]
    fn rejects_an_unknown_environment() {
        let error =
            Config::resolve(&args(&["--env", "staging"]), &no_env, Environment::Ddev).unwrap_err();

        assert!(error.to_string().contains("Unknown environment"));
    }

    #[test]
    fn rejects_a_missing_flag_value() {
        let error = Config::resolve(&args(&["--env"]), &no_env, Environment::Ddev).unwrap_err();

        assert!(error.to_string().contains("needs a value"));
    }

    #[test]
    fn rejects_a_nonsense_port() {
        let error =
            Config::resolve(&args(&["--port", "abc"]), &no_env, Environment::Ddev).unwrap_err();

        assert!(error.to_string().contains("is not a port"));
    }

    #[test]
    fn keeps_a_pre_registered_client_id() {
        let config = Config::resolve(
            &[],
            &|key| (key == "LYNXBY_CLIENT_ID").then(|| "abc".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.client_id.as_deref(), Some("abc"));
    }

    #[test]
    fn an_empty_client_id_is_no_client_id() {
        let config = Config::resolve(
            &[],
            &|key| (key == "LYNXBY_CLIENT_ID").then(|| "  ".to_string()),
            Environment::Ddev,
        )
        .unwrap();

        assert_eq!(config.client_id, None);
    }

    #[test]
    fn reports_the_host_for_the_login_screen() {
        let config = Config::resolve(&[], &no_env, Environment::Ddev).unwrap();

        assert_eq!(config.host(), "projectsovereign.ddev.site");
    }

    #[test]
    fn help_is_an_error_that_says_how_to_start() {
        let error = Config::resolve(&args(&["--help"]), &no_env, Environment::Ddev).unwrap_err();

        assert!(error.to_string().contains("Usage: lynxby-desktop"));
    }
}
