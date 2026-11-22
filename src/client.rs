//! Authenticated, read-only access to the customer area (`my.fidelidade.pt`).

use std::fmt;
use std::sync::LazyLock;

use chrono::{DateTime, Utc};
use regex::Regex;
use reqwest::header;

use crate::config::Config;
use crate::error::{Error, Result};
use crate::html;
use crate::model::{
    Document, Download, Handle, Policy, PolicyDetail, Receipt, ReceiptDetail, RiskUnit,
};
use crate::session::{Http, Jar, Page, SessionState};
use crate::{home, pages, policy, receipts};

/// The "Os Meus Documentos" page: every document of every policy.
pub(crate) const DOCUMENTS_PATH: &str = "CAND_AreaCliente/AC_Documentos.aspx";
/// The home page: one card per policy.
const HOME_PATH: &str = "CAND_AreaCliente/HomePage.aspx";
/// "Recibos a pagamento".
const RECEIPTS_PATH: &str = "CAND_AC_Apolices/PaymentReceiptList.aspx";

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

    /// The user's policies, as listed in the documents page's insurance filter: name, number
    /// and product family. One request, and the cheap one to use when that is all you need;
    /// [`Client::policies_with_details`] adds what the home page knows.
    pub async fn policies(&self) -> Result<Vec<Policy>> {
        let (page, form) = self.documents_page().await?;
        Ok(pages::policies(&html::parse(&page.body), &form))
    }

    /// Like [`Client::policies`], plus the renewal date, agent, number of insured units and a
    /// receipt due date, which the home page's cards show (two more requests). A policy the home
    /// page does not list, or cannot be told apart on it, keeps `None` in those fields.
    pub async fn policies_with_details(&self) -> Result<Vec<Policy>> {
        let mut policies = self.policies().await?;
        let (home_page, home_form) = self.home_page().await?;
        let cards = home::cards(&html::parse(&home_page.body));
        if cards.is_empty() && !policies.is_empty() {
            return Err(Error::unexpected(
                "reading the home page",
                "it lists no policies although the documents page does",
            ));
        }
        home::enrich(&mut policies, &cards, &home_form);
        Ok(policies)
    }

    /// The "Dados gerais" of a policy: dates, agent, premium and how it is paid. The IBAN,
    /// the direct-debit authorization and the policyholder's details are not read.
    ///
    /// Policies are opened through the home page, and the portal remembers the policy opened
    /// last for the whole session, so each call walks the whole chain: do not reuse the
    /// pages' addresses.
    pub async fn policy_detail(&self, policy: &Policy) -> Result<PolicyDetail> {
        let policy = self.with_links(policy).await?;
        let handle = policy
            .detail
            .as_ref()
            .ok_or_else(|| no_home_card(&policy))?;
        let page = self.open(handle).await?;
        Ok(policy::detail(&html::parse(&page.body)))
    }

    /// The insured units of a policy, each with its data and what it is covered for. Home
    /// insurance has one unit per dwelling and one for its contents, each with a list of
    /// coverages (capital and deductible).
    ///
    /// A policy whose page has no unit selector gives a single unit named after the product.
    /// For life policies that is the portal's "Universo Seguro" page: the insured person's data
    /// and the balance, if any, in [`RiskUnit::info`] and no coverages. Both the address of a
    /// home and a person's name and birth date are personal data.
    pub async fn coverages(&self, policy: &Policy) -> Result<Vec<RiskUnit>> {
        let policy = self.with_links(policy).await?;
        let handle = policy
            .coverage
            .as_ref()
            .ok_or_else(|| no_home_card(&policy))?;
        let page = self.open(handle).await?;
        let doc = html::parse(&page.body);
        let tabs = policy::unit_tabs(&doc);
        if tabs.is_empty() {
            return Ok(policy::unit(&doc, &policy.name).into_iter().collect());
        }
        let form = pages::form(&doc, &page.url)?;
        let mut units = Vec::new();
        for tab in tabs {
            let unit = if tab.active {
                policy::unit(&doc, &tab.name)
            } else {
                let Some(target) = tab.target else { continue };
                let other = Handle {
                    form: form.clone(),
                    target,
                };
                let page = self.post_back(&other, &page.url).await?;
                policy::unit(&html::parse(&page.body), &tab.name)
            };
            units.extend(unit);
        }
        Ok(units)
    }

    /// The premium receipts waiting to be paid ("Recibos a pagamento"), with their amount,
    /// due date and payment method.
    pub async fn receipts_due(&self) -> Result<Vec<Receipt>> {
        let page = self.http.get(self.http.config.area(RECEIPTS_PATH)).await?;
        let page = self.check(page)?;
        if !page.url.path().ends_with("PaymentReceiptList.aspx") {
            return Err(Error::unexpected(
                "loading the receipts",
                format!("the portal sent us to {}", page.url.path()),
            ));
        }
        let doc = html::parse(&page.body);
        let form = pages::form(&doc, &page.url)?;
        receipts::list(&doc, &form)
    }

    /// The detail page of a receipt: issue and due dates, periods and the Multibanco reference
    /// to pay it by hand. The IBAN and the direct-debit authorization are not read.
    pub async fn receipt(&self, receipt: &Receipt) -> Result<ReceiptDetail> {
        let page = self.open(&receipt.handle).await?;
        Ok(receipts::detail(&html::parse(&page.body)))
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
        let doc = html::parse(&page.body);
        // The insurance filter is always there. After the portal renews a session it sometimes
        // serves the page without it, and reading that as "no policies" would be a lie.
        if doc
            .select(&html::selector(".ButtonDropdown_text"))
            .next()
            .is_none()
        {
            return Err(Error::unexpected(
                "reading the documents page",
                "the page came back without its insurance filter",
            ));
        }
        let form = pages::form(&doc, &page.url)?;
        Ok((page, form))
    }

    /// `policy` with the links of its home card: it already has them when it comes from
    /// [`Client::policies_with_details`], otherwise the home page is read to find them.
    async fn with_links(&self, policy: &Policy) -> Result<Policy> {
        if policy.detail.is_some() || policy.coverage.is_some() {
            return Ok(policy.clone());
        }
        let found = self
            .policies_with_details()
            .await?
            .into_iter()
            .find(|p| p.number == policy.number);
        found.ok_or_else(|| no_home_card(policy))
    }

    /// The home page, with the policies the "Ver mais seguros" button hides.
    async fn home_page(&self) -> Result<(Page, std::sync::Arc<html::Form>)> {
        let page = self.http.get(self.http.config.area(HOME_PATH)).await?;
        let page = self.check(page)?;
        let doc = html::parse(&page.body);
        let form = pages::form(&doc, &page.url)?;
        let Some(target) = home::more_target(&doc) else {
            return Ok((page, form));
        };
        let more = Handle {
            form: form.clone(),
            target,
        };
        let expanded = self.post_back(&more, &page.url).await?;
        let expanded_form = pages::form(&html::parse(&expanded.body), &expanded.url)?;
        Ok((expanded, expanded_form))
    }

    /// Follow a link of the home or receipts page; the portal redirects to the page it opens.
    async fn open(&self, handle: &Handle) -> Result<Page> {
        self.post_back(handle, &handle.form.action).await
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
        // The portal's own error page is served with a 200.
        if page.url.path().contains("/Exceptions.") {
            return Err(Error::unexpected(
                "loading a page",
                format!(
                    "the portal answered with its error page ({})",
                    page.url.path()
                ),
            ));
        }
        Ok(page)
    }
}

fn no_home_card(policy: &Policy) -> Error {
    Error::InvalidInput(format!(
        "the home page does not list {:?} in a way that can be matched to it",
        policy.name
    ))
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
