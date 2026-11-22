use std::time::Duration;

use url::Url;

const DEFAULT_USER_AGENT: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) \
     AppleWebKit/537.36 (KHTML, like Gecko) Chrome/141.0.0.0 Safari/537.36";

/// Where the customer area lives and how to talk to it.
///
/// The defaults point at production. Overriding the URL is mostly useful for
/// tests (see `tests/flow.rs`) or a recording proxy. The single sign-on host
/// (`autenticacao.fidelidade.pt`) needs no setting: the customer area redirects
/// to it, and the crate follows wherever it is sent.
#[derive(Clone, Debug)]
pub struct Config {
    /// Base URL of the customer area (`https://www.my.fidelidade.pt/`).
    pub base_url: Url,
    /// `User-Agent` sent with every request.
    pub user_agent: String,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            base_url: Url::parse("https://www.my.fidelidade.pt/").expect("valid url"),
            user_agent: DEFAULT_USER_AGENT.to_owned(),
            timeout: Duration::from_secs(60),
        }
    }
}

impl Config {
    pub(crate) fn area(&self, path: &str) -> Url {
        self.base_url.join(path).expect("static path joins")
    }

    /// Whether `url` is on the customer area's origin (as opposed to the login host).
    pub(crate) fn is_area(&self, url: &Url) -> bool {
        url.origin() == self.base_url.origin()
    }
}
