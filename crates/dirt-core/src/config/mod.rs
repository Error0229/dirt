//! Bootstrap configuration for client apps.
//!
//! `BootstrapConfig` carries the build-baked API base URL clients need
//! at startup. Phase-1's runtime "bootstrap manifest" (`/v1/bootstrap`
//! endpoint, [`ManagedBootstrapManifest`], runtime fetch) was deleted
//! when the server-side route was removed; clients now read the
//! embedded JSON straight from `build.rs` and never hit the network at
//! startup. Bearer-token secrets live in the OS keychain (clients)
//! or in `DIRT_SERVER_TOKEN` (server), never here.

use serde::{Deserialize, Serialize};

use crate::util::normalize_text_option;

/// Build-provisioned client configuration.
///
/// Safe-to-ship public endpoints required to bootstrap the API client.
/// Secret credentials must never be stored here.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct BootstrapConfig {
    #[serde(default)]
    pub dirt_api_base_url: Option<String>,
}

impl BootstrapConfig {
    /// Returns the dirt-api base URL when configured.
    pub fn managed_api_base_url(&self) -> Option<String> {
        normalize_text_option(self.dirt_api_base_url.clone())
    }
}

/// The environment variable that names the server.
pub const SERVER_ADDRESS_VARIABLE: &str = "DIRT_API_BASE_URL";

/// Where a program's server address came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressSource {
    /// `DIRT_API_BASE_URL` was set when the program started.
    Environment,
    /// The address the program was built with.
    Build,
}

/// The server a program on a computer syncs with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddress {
    pub url: String,
    pub source: AddressSource,
}

/// A server address that cannot be used.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error(
        "{SERVER_ADDRESS_VARIABLE} is set to {0:?}, which is not an address that starts with http:// or https://"
    )]
    EnvironmentIsNotAnAddress(String),
    #[error(
        "this program was built with the server address {0:?}, which does not start with http:// or https://"
    )]
    BuildIsNotAnAddress(String),
}

impl ServerAddress {
    /// The server for a program on a computer: the desktop app or the CLI.
    ///
    /// `DIRT_API_BASE_URL` in the environment comes first, and the address
    /// the program was built with second. The two programs on one computer
    /// are one device, with one keychain entry named after the server, so
    /// both use this rule and neither has another.
    pub fn on_this_computer(built_in: Option<&str>) -> Result<Option<Self>, ConfigError> {
        Self::choose(std::env::var(SERVER_ADDRESS_VARIABLE).ok(), built_in)
    }

    /// The server for the Android app: the address it was built with.
    /// Android gives an app no environment to set.
    ///
    /// `.env.client` serves every client and usually names a server on
    /// the computer as `127.0.0.1` or `localhost`. On a phone those are
    /// the phone. They become `10.0.2.2`, which is the emulator's name for
    /// the computer it runs on. An address of any other machine is kept.
    pub fn on_android(built_in: Option<&str>) -> Result<Option<Self>, ConfigError> {
        let Some(given) = normalize_text_option(built_in.map(str::to_owned)) else {
            return Ok(None);
        };
        let url = with_host_renamed(&given, &["127.0.0.1", "localhost"], "10.0.2.2");
        if !crate::util::is_http_url(&url) {
            return Err(ConfigError::BuildIsNotAnAddress(given));
        }
        Ok(Some(Self {
            url,
            source: AddressSource::Build,
        }))
    }

    /// The rule itself, apart from where the environment's value is read.
    pub fn choose(
        from_environment: Option<String>,
        built_in: Option<&str>,
    ) -> Result<Option<Self>, ConfigError> {
        if let Some(given) = normalize_text_option(from_environment) {
            let url = on_a_computer(&given);
            if !crate::util::is_http_url(&url) {
                return Err(ConfigError::EnvironmentIsNotAnAddress(given));
            }
            return Ok(Some(Self {
                url,
                source: AddressSource::Environment,
            }));
        }
        let Some(given) = normalize_text_option(built_in.map(str::to_owned)) else {
            return Ok(None);
        };
        let url = on_a_computer(&given);
        if !crate::util::is_http_url(&url) {
            return Err(ConfigError::BuildIsNotAnAddress(given));
        }
        Ok(Some(Self {
            url,
            source: AddressSource::Build,
        }))
    }
}

/// An address as a computer reaches it. One `.env.client` serves every
/// client, and may name the computer the way the Android emulator does
/// (`10.0.2.2`) or as `localhost`. Both become `127.0.0.1`, so that the
/// desktop app and the CLI name the server alike whichever was written.
fn on_a_computer(address: &str) -> String {
    with_host_renamed(address, &["10.0.2.2", "localhost"], "127.0.0.1")
}

/// `address` with its host replaced by `to` if the host is one of `from`.
///
/// Only a host that is exactly one of those names is changed. `10.0.2.20`
/// and `localhost.example` are other machines, and everything else in the
/// address is kept as it was written.
fn with_host_renamed(address: &str, from: &[&str], to: &str) -> String {
    let address = address.trim().trim_end_matches('/');
    let Some((scheme, rest)) = address.split_once("://") else {
        return address.to_owned();
    };
    // The host sits between any "user@" and the port, path, query or
    // fragment, whichever comes first.
    let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
    let (authority, tail) = rest.split_at(authority_end);
    let (user, host_and_port) = authority
        .rfind('@')
        .map_or(("", authority), |at| authority.split_at(at + 1));
    // An IPv6 address is written in brackets and is neither name.
    if host_and_port.starts_with('[') {
        return address.to_owned();
    }
    let host_end = host_and_port.find(':').unwrap_or(host_and_port.len());
    let (host, port) = host_and_port.split_at(host_end);
    if from.iter().any(|name| host.eq_ignore_ascii_case(name)) {
        format!("{scheme}://{user}{to}{port}{tail}")
    } else {
        address.to_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_environment_comes_before_the_address_built_in() {
        let chosen = ServerAddress::choose(
            Some(" https://mine.example/ ".to_owned()),
            Some("https://built.example"),
        )
        .unwrap()
        .unwrap();
        assert_eq!(chosen.url, "https://mine.example");
        assert_eq!(chosen.source, AddressSource::Environment);

        for unset in [None, Some(String::new()), Some("   ".to_owned())] {
            let chosen = ServerAddress::choose(unset, Some("https://built.example/"))
                .unwrap()
                .unwrap();
            assert_eq!(chosen.url, "https://built.example");
            assert_eq!(chosen.source, AddressSource::Build);
        }
        assert_eq!(ServerAddress::choose(None, None).unwrap(), None);
        assert_eq!(ServerAddress::choose(None, Some("  ")).unwrap(), None);
    }

    #[test]
    fn the_computer_itself_has_one_name() {
        for written in [
            "http://localhost:8080",
            "http://10.0.2.2:8080/",
            "http://127.0.0.1:8080",
        ] {
            let from_build = ServerAddress::choose(None, Some(written)).unwrap().unwrap();
            let from_environment = ServerAddress::choose(Some(written.to_owned()), None)
                .unwrap()
                .unwrap();
            assert_eq!(from_build.url, "http://127.0.0.1:8080");
            assert_eq!(from_environment.url, "http://127.0.0.1:8080");
        }
    }

    #[test]
    fn only_a_whole_host_name_is_taken_for_the_computer_itself() {
        let unchanged = [
            // Other machines whose names begin the same way.
            "http://10.0.2.20:8080",
            "http://10.0.2.2.example:8080",
            "https://localhost.example",
            "https://localhostess",
            // The names somewhere other than the host.
            "https://sync.example/localhost",
            "https://sync.example/?next=http://localhost",
            "https://localhost@sync.example",
            "http://[::1]:8080",
        ];
        for written in unchanged {
            assert_eq!(on_a_computer(written), written);
        }
        let changed = [
            ("http://localhost", "http://127.0.0.1"),
            ("http://LocalHost:8080/", "http://127.0.0.1:8080"),
            ("http://10.0.2.2", "http://127.0.0.1"),
            ("http://10.0.2.2:8080/api", "http://127.0.0.1:8080/api"),
            ("http://me@localhost:8080", "http://me@127.0.0.1:8080"),
            ("http://localhost?x=1", "http://127.0.0.1?x=1"),
        ];
        for (written, reached) in changed {
            assert_eq!(on_a_computer(written), reached);
        }
    }

    #[test]
    fn the_android_app_reaches_the_computer_by_the_emulators_name_for_it() {
        let reached = |written: &str| {
            ServerAddress::on_android(Some(written))
                .unwrap()
                .unwrap()
                .url
        };
        assert_eq!(reached("http://127.0.0.1:8080/"), "http://10.0.2.2:8080");
        assert_eq!(reached("http://localhost:8080"), "http://10.0.2.2:8080");
        assert_eq!(reached("http://10.0.2.2:8080"), "http://10.0.2.2:8080");
        // Other machines, whatever their names begin with.
        assert_eq!(reached("http://127.0.0.10:8080"), "http://127.0.0.10:8080");
        assert_eq!(
            reached("https://localhost.example"),
            "https://localhost.example"
        );
        assert_eq!(reached("https://sync.example"), "https://sync.example");

        assert_eq!(ServerAddress::on_android(None).unwrap(), None);
        assert_eq!(ServerAddress::on_android(Some(" ")).unwrap(), None);
        assert_eq!(
            ServerAddress::on_android(Some("sync.example")),
            Err(ConfigError::BuildIsNotAnAddress("sync.example".to_owned()))
        );
    }

    #[test]
    fn something_that_is_not_an_address_is_refused_and_not_passed_over() {
        // A mistyped address must not fall back to another server without
        // a word: the notes would go somewhere the person did not mean.
        assert_eq!(
            ServerAddress::choose(
                Some("mine.example".to_owned()),
                Some("https://built.example")
            ),
            Err(ConfigError::EnvironmentIsNotAnAddress(
                "mine.example".to_owned()
            ))
        );
        assert_eq!(
            ServerAddress::choose(None, Some("built.example")),
            Err(ConfigError::BuildIsNotAnAddress("built.example".to_owned()))
        );
    }

    #[test]
    fn managed_api_base_url_returns_configured_value() {
        let config = BootstrapConfig {
            dirt_api_base_url: Some("https://api.example.com".to_string()),
        };
        assert_eq!(
            config.managed_api_base_url().as_deref(),
            Some("https://api.example.com")
        );
    }

    #[test]
    fn managed_api_base_url_normalizes_empty() {
        let config = BootstrapConfig {
            dirt_api_base_url: Some("   ".to_string()),
        };
        assert!(config.managed_api_base_url().is_none());
    }
}
