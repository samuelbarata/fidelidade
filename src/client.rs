//! Authenticated, read-only access to the customer area (`my.fidelidade.pt`).

use std::fmt;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::header;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::html;
use crate::model::{Document, Download, Handle, Policy};
use crate::pages;
use crate::session::{Http, Jar, Page, SessionState};

/// The "Os Meus Documentos" page: every document of every policy.
pub(crate) const DOCUMENTS_PATH: &str = "CAND_AreaCliente/AC_Documentos.aspx";

/// A logged-in session.
///
/// Get one from [`Authenticator::login`](crate::Authenticator::login), or restore a
/// saved one with [`Client::from_session`]. Cloning is cheap and clones share the
/// session. Everything here only reads: nothing changes the user's account.
#[derive(Clone)]
pub struct Client {
    http: Http,
    nif: String,
    authenticated_at: DateTime<Utc>,
}

impl fmt::Debug for Client {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Client")
            .field("nif", &self.nif)
            .field("authenticated_at", &self.authenticated_at)
            .finish_non_exhaustive()
    }
}

impl Client {
    pub(crate) fn new(http: Http, nif: String, authenticated_at: DateTime<Utc>) -> Self {
        Self {
            http,
            nif,
            authenticated_at,
        }
    }

    /// Restore a session saved with [`Client::session`]. Does not hit the network; call
    /// [`Client::is_authenticated`] to check it is still alive.
    pub fn from_session(state: SessionState, config: Config) -> Result<Self> {
        let http = Http::new(config, Jar::from_state(&state.cookies)?)?;
        Ok(Self::new(http, state.nif, state.authenticated_at))
    }

    /// Snapshot of the session (cookies). Contains credentials-equivalent secrets:
    /// encrypt it at rest.
    pub fn session(&self) -> Result<SessionState> {
        Ok(SessionState {
            nif: self.nif.clone(),
            authenticated_at: self.authenticated_at,
            cookies: self.http.jar.to_state()?,
        })
    }

    /// NIF the session belongs to.
    pub fn nif(&self) -> &str {
        &self.nif
    }

    /// When the login completed.
    pub fn authenticated_at(&self) -> DateTime<Utc> {
        self.authenticated_at
    }

    /// `true` if the portal still accepts this session (the documents page loads
    /// without bouncing to the login page).
    pub async fn is_authenticated(&self) -> Result<bool> {
        let page = self.http.get(self.http.config.area(DOCUMENTS_PATH)).await?;
        Ok(self.http.config.is_area(&page.url) && page.status.is_success())
    }

    /// The user's policies, as listed in the documents page's insurance filter.
    pub async fn policies(&self) -> Result<Vec<Policy>> {
        let (page, form) = self.documents_page().await?;
        Ok(pages::policies(&html::parse(&page.body), &form))
    }

    /// Documents, newest first as the portal orders them: all of them with `None`, or
    /// only those of `policy`.
    ///
    /// The "all" view also holds documents that belong to no policy (capitalisation
    /// statements, letters), so it is not just the union of the per-policy views. The
    /// portal showed no pagination at the time of writing; if an account ever has more
    /// documents than one page holds, the per-policy views are the way to reach them.
    pub async fn documents(&self, policy: Option<&Policy>) -> Result<Vec<Document>> {
        let (page, form) = self.documents_page().await?;
        let Some(policy) = policy else {
            return pages::documents(&html::parse(&page.body), &form);
        };
        let page = self.post_back(&policy.handle, &page.url).await?;
        let form = pages::form(&html::parse(&page.body), &page.url)?;
        pages::documents(&html::parse(&page.body), &form)
    }

    /// Download a document, as the "Ver documento" button does.
    pub async fn download(&self, document: &Document) -> Result<Download> {
        let Handle { form, target } = &document.handle;
        let response = self
            .http
            .post_form_raw(
                form.action.clone(),
                &form.post_back(target),
                Some(&self.http.config.area(DOCUMENTS_PATH)),
            )
            .await?;
        if !self.http.config.is_area(response.url()) {
            return Err(Error::SessionExpired);
        }
        let status = response.status();
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|v| v.to_str().ok())
            .unwrap_or_default()
            .to_owned();
        let filename = response
            .headers()
            .get(header::CONTENT_DISPOSITION)
            .and_then(|v| v.to_str().ok())
            .and_then(filename_from_disposition);
        if !status.is_success() || content_type.starts_with("text/html") {
            return Err(Error::unexpected(
                "downloading a document",
                format!("status {status}, content-type {content_type:?}: expected a file"),
            ));
        }
        let bytes = response.bytes().await?.to_vec();
        tracing::debug!(len = bytes.len(), "downloaded");
        Ok(Download {
            filename: filename.unwrap_or_else(|| "document".to_owned()),
            content_type,
            bytes,
        })
    }

    /// Load the documents page and its form. A bounce to the login host means the
    /// session is gone.
    async fn documents_page(&self) -> Result<(Page, std::sync::Arc<html::Form>)> {
        let page = self.http.get(self.http.config.area(DOCUMENTS_PATH)).await?;
        let page = self.check(page)?;
        let form = pages::form(&html::parse(&page.body), &page.url)?;
        Ok((page, form))
    }

    async fn post_back(&self, handle: &Handle, referer: &url::Url) -> Result<Page> {
        let page = self
            .http
            .post_form(
                handle.form.action.clone(),
                &handle.form.post_back(&handle.target),
                Some(referer),
            )
            .await?;
        self.check(page)
    }

    fn check(&self, page: Page) -> Result<Page> {
        if !self.http.config.is_area(&page.url) {
            return Err(Error::SessionExpired);
        }
        if !page.status.is_success() {
            return Err(Error::unexpected(
                "loading a page",
                format!("status {}", page.status),
            ));
        }
        Ok(page)
    }
}

/// `attachment; filename="Fatura-Recibo.pdf"` → `Fatura-Recibo.pdf`.
fn filename_from_disposition(value: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"filename\s*=\s*"?([^";]+)"?"#).unwrap());
    let name = RE.captures(value)?.get(1)?.as_str().trim();
    // Never let the portal pick a path.
    let name = name.rsplit(['/', '\\']).next().unwrap_or(name);
    (!name.is_empty()).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn filenames() {
        assert_eq!(
            filename_from_disposition(r#"attachment; filename="Fatura-Recibo.pdf""#).as_deref(),
            Some("Fatura-Recibo.pdf")
        );
        assert_eq!(
            filename_from_disposition("attachment; filename=a.pdf; size=1").as_deref(),
            Some("a.pdf")
        );
        assert_eq!(
            filename_from_disposition(r#"attachment; filename="../../etc/passwd""#).as_deref(),
            Some("passwd")
        );
        assert_eq!(filename_from_disposition("inline"), None);
    }
}
