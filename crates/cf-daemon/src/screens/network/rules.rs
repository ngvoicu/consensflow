//! The rules of clippy.toml for HTTPS clients (T-1253), held by fixtures.
//!
//! The workspace's reqwest is `rustls-no-provider`: a client panics at its build where the process
//! has installed no TLS provider (the Windows update check did, T-1247). So a client is built only
//! by its crate's approved constructor (`client` in `super`, and `update_page::client` in the app),
//! and clippy refuses every other construction.
//!
//! Each function below builds a client the way a change might, outside that constructor, and
//! `expect`s the lints that refuse it. Clippy reports an expectation its lint did not meet, so a
//! rule dropped from clippy.toml, or a shape it stops recognising (a path that moved in a new
//! reqwest or rustls), fails the gate here: the rules are held by the lint's own word. Nothing here
//! is called, or is meant to run.
#![allow(dead_code, unused_imports)] // Read by clippy, never called.

/// Rules that name a method.
mod methods {
    #[expect(clippy::disallowed_methods, clippy::disallowed_types)]
    fn client_new() {
        let _ = reqwest::Client::new();
    }

    #[expect(clippy::disallowed_methods, clippy::disallowed_types)]
    fn client_builder() {
        let _ = reqwest::Client::builder().build();
    }

    #[expect(clippy::disallowed_methods, clippy::disallowed_types)]
    fn client_builder_new() {
        let _ = reqwest::ClientBuilder::new().build();
    }

    #[expect(clippy::disallowed_methods)]
    async fn the_free_get() {
        let _ = reqwest::get("https://example.invalid/").await;
    }

    /// A name for the type, or for the crate, is no way round.
    #[expect(clippy::disallowed_methods, clippy::disallowed_types)]
    fn an_aliased_client() {
        use reqwest::Client as Http;
        let _ = Http::new();
    }

    #[expect(clippy::disallowed_methods)]
    async fn an_aliased_crate() {
        use reqwest as http;
        let _ = http::get("https://example.invalid/").await;
    }

    #[expect(clippy::disallowed_methods, clippy::disallowed_types)]
    fn a_constructor_taken_as_a_function() {
        let make = reqwest::Client::new;
        let _ = make();
    }

    #[expect(clippy::disallowed_methods)]
    fn a_config_on_the_default_provider() {
        let _ = rustls::ClientConfig::builder();
    }

    #[expect(clippy::disallowed_methods)]
    fn a_config_on_the_default_provider_with_versions() {
        let _ = rustls::ClientConfig::builder_with_protocol_versions(&[]);
    }

    #[expect(clippy::disallowed_methods)]
    fn a_platform_verifier_config_on_the_default_provider() {
        use rustls_platform_verifier::ConfigVerifierExt;
        let _ = rustls::ClientConfig::with_platform_verifier();
    }
}

/// Rules that name a type: the `Default` impl of reqwest's types builds a client on no provider,
/// and has no path to name as a method, so the type itself is how it is refused.
mod types {
    #[expect(clippy::disallowed_types)]
    fn a_default_client() {
        let _ = reqwest::Client::default();
    }

    #[expect(clippy::disallowed_types)]
    fn a_default_client_by_its_type() -> reqwest::Client {
        Default::default()
    }

    #[expect(clippy::disallowed_types)]
    fn a_default_client_builder() {
        let _ = reqwest::ClientBuilder::default();
    }

    #[expect(clippy::disallowed_types)]
    #[derive(Default)]
    struct HoldsOne {
        client: reqwest::Client,
    }

    #[expect(clippy::disallowed_types)]
    type Http = reqwest::Client;

    #[expect(clippy::disallowed_types)]
    use reqwest::Client as Imported;
}
