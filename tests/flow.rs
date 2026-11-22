//! End-to-end flows against two mock servers standing in for `my.fidelidade.pt` (the
//! customer area) and `autenticacao.fidelidade.pt` (the single sign-on host). The pages
//! are synthetic, in the shapes the parsers match. Nothing here reaches the real portal.

use fidelidade::{Authenticator, Client, Config, Credentials, Error, SessionState};
use wiremock::matchers::{body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LOGIN: &str = include_str!("fixtures/login.html");
const LOGIN_WRONG: &str = include_str!("fixtures/login_wrong_password.html");
const DOCS_ALL: &str = include_str!("fixtures/documents_all.html");
const DOCS_POLICY: &str = include_str!("fixtures/documents_policy.html");
const DOCS: &str = "/CAND_AreaCliente/AC_Documentos.aspx";
const POST: &str = "/CAND_AreaCliente/DocumentosEntidade.aspx";
const PDF: &[u8] = b"%PDF-1.4\n% synthetic\n";

fn html(body: &str) -> ResponseTemplate {
    ResponseTemplate::new(200).set_body_raw(body.to_owned(), "text/html;charset=utf-8")
}

struct Servers {
    area: MockServer,
    sso: MockServer,
}

impl Servers {
    /// The area wants the `area_sid=ok` cookie; without it, it bounces to the login host.
    async fn start() -> Self {
        let s = Self {
            area: MockServer::start().await,
            sso: MockServer::start().await,
        };

        Mock::given(method("GET"))
            .and(path(DOCS))
            .and(header_regex("cookie", "area_sid=ok"))
            .respond_with(html(DOCS_ALL))
            .with_priority(1)
            .mount(&s.area)
            .await;
        Mock::given(method("GET"))
            .and(path(DOCS))
            .respond_with(ResponseTemplate::new(302).insert_header(
                "Location",
                format!("{}/Autenticacao/Authorize.aspx?client_id=x", s.sso.uri()),
            ))
            .with_priority(5)
            .mount(&s.area)
            .await;

        // Filtering by policy and downloading are both postbacks to the same URL.
        Mock::given(method("POST"))
            .and(path(POST))
            .and(body_string_contains("wtLRPoliciesCombo%24ctl02%24wt36"))
            .respond_with(html(DOCS_POLICY))
            .mount(&s.area)
            .await;
        Mock::given(method("POST"))
            .and(path(POST))
            .and(body_string_contains("Docs%24ctl00%24Open"))
            .and(body_string_contains("state-all"))
            .respond_with(
                ResponseTemplate::new(200)
                    .insert_header(
                        "Content-Disposition",
                        "attachment; filename=\"Fatura-Recibo.pdf\"",
                    )
                    .set_body_raw(PDF.to_vec(), "application/octet-stream"),
            )
            .mount(&s.area)
            .await;

        Mock::given(method("GET"))
            .and(path("/canw_auth/SSOEntryPage.aspx"))
            .respond_with(
                ResponseTemplate::new(302)
                    .insert_header("Set-Cookie", "area_sid=ok; Path=/; HttpOnly")
                    .insert_header("Location", DOCS),
            )
            .mount(&s.area)
            .await;

        Mock::given(method("GET"))
            .and(path("/Autenticacao/Authorize.aspx"))
            .respond_with(html(LOGIN).insert_header("Set-Cookie", "sso_sid=1; Path=/; HttpOnly"))
            .mount(&s.sso)
            .await;
        s
    }

    /// Correct password: 302 to the area's SSO entry page. Anything else: the login page again.
    async fn mount_login(&self) {
        Mock::given(method("POST"))
            .and(path("/Autenticacao/Login.aspx"))
            .and(body_string_contains("123456789"))
            .and(body_string_contains("correct+horse"))
            // Clicks the login button, replaying the page state it was served with.
            .and(body_string_contains(
                "__EVENTTARGET=Layout%24block%24Login%24block%24wtlnk_Login",
            ))
            .and(body_string_contains("__OSVSTATE=login-state"))
            // "Keep me signed in" ticked.
            .and(body_string_contains("wtCheckbox_IsPersistentLogin=on"))
            .respond_with(ResponseTemplate::new(302).insert_header(
                "Location",
                format!("{}/canw_auth/SSOEntryPage.aspx?code=abc", self.area.uri()),
            ))
            .with_priority(1)
            .mount(&self.sso)
            .await;
        Mock::given(method("POST"))
            .and(path("/Autenticacao/Login.aspx"))
            .respond_with(html(LOGIN_WRONG))
            .with_priority(5)
            .mount(&self.sso)
            .await;
    }

    fn config(&self) -> Config {
        Config {
            base_url: self.area.uri().parse().unwrap(),
            ..Config::default()
        }
    }

    async fn client(&self) -> Client {
        self.mount_login().await;
        let credentials = Credentials::new("123456789", "correct horse").unwrap();
        Authenticator::with_config(self.config())
            .login(&credentials)
            .await
            .expect("login")
    }
}

#[tokio::test]
async fn logs_in_and_the_session_survives_a_round_trip() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    assert_eq!(client.nif(), "123456789");
    assert!(client.is_authenticated().await.unwrap());

    // Serialize, restore into a brand new client (as another process would), still logged in.
    let stored = serde_json::to_string(&client.session().unwrap()).unwrap();
    let state: SessionState = serde_json::from_str(&stored).unwrap();
    let restored = Client::from_session(state, servers.config()).unwrap();
    assert!(restored.is_authenticated().await.unwrap());
}

#[tokio::test]
async fn wrong_password_is_an_auth_failure_with_the_portals_message() {
    let servers = Servers::start().await;
    servers.mount_login().await;
    let credentials = Credentials::new("123456789", "wrong").unwrap();
    let err = Authenticator::with_config(servers.config())
        .login(&credentials)
        .await
        .unwrap_err();
    assert!(err.is_auth_failure(), "{err:?}");
    assert!(err.to_string().contains("inválidos"), "{err}");
    assert!(!err.to_string().contains("wrong"));
}

#[tokio::test]
async fn lists_policies() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let policies = client.policies().await.unwrap();
    // The "TODOS" entry is not a policy.
    assert_eq!(policies.len(), 2);
    assert_eq!(policies[0].name, "PPR Teste");
    assert_eq!(policies[0].number, "0000000001");
    assert_eq!(policies[0].category.as_deref(), Some("Vida"));
    assert_eq!(policies[1].number, "MR00000002");
    assert_eq!(policies[1].category.as_deref(), Some("Casa"));
}

#[tokio::test]
async fn lists_all_documents_with_stable_distinct_ids() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let docs = client.documents(None).await.unwrap();
    assert_eq!(docs.len(), 4);

    assert_eq!(docs[0].kind, "Fatura-Recibo");
    assert_eq!(docs[0].date.to_string(), "2026-11-16");
    assert_eq!(docs[0].policy_number.as_deref(), Some("MR00000002"));
    assert_eq!(docs[0].id(), "MR00000002:2026-11-16:fatura-recibo:0");

    // A document that belongs to no policy.
    assert_eq!(docs[1].policy_number, None);
    assert_eq!(
        docs[1].id(),
        "-:2026-11-12:extrato-de-seguros-de-capitalizacao-e-ppr:0"
    );

    // Two identical entries stay distinguishable.
    assert_eq!(
        docs[2].id(),
        "0000000001:2026-10-12:aviso-recibo-de-premio:0"
    );
    assert_eq!(
        docs[3].id(),
        "0000000001:2026-10-12:aviso-recibo-de-premio:1"
    );

    // Listing again gives the same ids.
    let again: Vec<_> = client
        .documents(None)
        .await
        .unwrap()
        .iter()
        .map(|d| d.id())
        .collect();
    assert_eq!(again, docs.iter().map(|d| d.id()).collect::<Vec<_>>());
}

#[tokio::test]
async fn filters_documents_by_policy() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let policies = client.policies().await.unwrap();
    let docs = client.documents(Some(&policies[1])).await.unwrap();
    assert_eq!(docs.len(), 1);
    assert_eq!(docs[0].kind, "Condições Gerais");
    assert_eq!(docs[0].policy_number.as_deref(), Some("MR00000002"));
}

#[tokio::test]
async fn downloads_a_document() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let docs = client.documents(None).await.unwrap();
    let file = client.download(&docs[0]).await.unwrap();
    assert_eq!(file.filename, "Fatura-Recibo.pdf");
    assert!(file.is_pdf());
    assert_eq!(file.bytes, PDF);
}

#[tokio::test]
async fn an_expired_session_is_reported_as_such() {
    let servers = Servers::start().await;
    // A session with no cookies: the area bounces it to the login host.
    let state: SessionState = serde_json::from_value(serde_json::json!({
        "nif": "123456789",
        "authenticated_at": "2026-01-01T00:00:00Z",
        "cookies": [],
    }))
    .unwrap();
    let client = Client::from_session(state, servers.config()).unwrap();
    assert!(!client.is_authenticated().await.unwrap());
    let err = client.documents(None).await.unwrap_err();
    assert!(matches!(err, Error::SessionExpired), "{err:?}");
    assert!(err.needs_login());
}
