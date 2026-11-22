//! End-to-end flows against two mock servers standing in for `my.fidelidade.pt` (the
//! customer area) and `autenticacao.fidelidade.pt` (the single sign-on host). The pages
//! are synthetic, in the shapes the parsers match. Nothing here reaches the real portal.

use chrono::NaiveDate;
use fidelidade::{Authenticator, Capital, Client, Config, Credentials, Error, SessionState};
use wiremock::matchers::{body_string_contains, header_regex, method, path};
use wiremock::{Mock, MockServer, ResponseTemplate};

const LOGIN: &str = include_str!("fixtures/login.html");
const LOGIN_WRONG: &str = include_str!("fixtures/login_wrong_password.html");
const DOCS_ALL: &str = include_str!("fixtures/documents_all.html");
const DOCS_POLICY: &str = include_str!("fixtures/documents_policy.html");
const HOME: &str = include_str!("fixtures/home.html");
const HOME_MORE: &str = include_str!("fixtures/home_more.html");
const POLICY_DETAIL: &str = include_str!("fixtures/policy_detail.html");
const UNITS: &str = include_str!("fixtures/units.html");
const UNITS_2: &str = include_str!("fixtures/units_2.html");
const LIFE_UNITS: &str = include_str!("fixtures/life_units.html");
const RECEIPTS: &str = include_str!("fixtures/receipts.html");
const RECEIPT_DETAIL: &str = include_str!("fixtures/receipt_detail.html");
const HOME_PATH: &str = "/CAND_AreaCliente/HomePage.aspx";
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
        s.mount_policy_pages().await;
        s
    }

    /// Home page (with its "Ver mais seguros" button), the policy tabs and the receipts.
    /// Opening something is a postback that the portal answers with a redirect.
    async fn mount_policy_pages(&self) {
        let redirect =
            |to: &str| ResponseTemplate::new(302).insert_header("Location", to.to_owned());
        let get = |p: &str, body: &str| {
            Mock::given(method("GET"))
                .and(path(p.to_owned()))
                .respond_with(html(body))
        };
        let post = |p: &str, target: &str| {
            Mock::given(method("POST"))
                .and(path(p.to_owned()))
                .and(body_string_contains(target.to_owned()))
        };
        let a = &self.area;
        get(HOME_PATH, HOME).mount(a).await;
        post(HOME_PATH, "Home%24more")
            .respond_with(html(HOME_MORE))
            .mount(a)
            .await;
        post(HOME_PATH, "Home%24Casa%24Detail")
            .respond_with(redirect("/CAND_AC_Apolices/PolicyDetail.aspx?Guid=g1"))
            .mount(a)
            .await;
        post(HOME_PATH, "Home%24Casa%24Cov")
            .respond_with(redirect("/CAND_AC_Apolices/PolicyUnitAtRisk.aspx?Guid=g1"))
            .mount(a)
            .await;
        post(HOME_PATH, "Home%24Vida%24Cov")
            .respond_with(redirect("/CAND_AC_Apolices/PolicyUnitAtRisk.aspx?Guid=g2"))
            .mount(a)
            .await;
        get("/CAND_AC_Apolices/PolicyDetail.aspx", POLICY_DETAIL)
            .mount(a)
            .await;
        Mock::given(method("GET"))
            .and(path("/CAND_AC_Apolices/PolicyUnitAtRisk.aspx"))
            .and(wiremock::matchers::query_param("Guid", "g2"))
            .respond_with(html(LIFE_UNITS))
            .with_priority(1)
            .mount(a)
            .await;
        get("/CAND_AC_Apolices/PolicyUnitAtRisk.aspx", UNITS)
            .mount(a)
            .await;
        post(
            "/CAND_AC_Apolices/PolicyUnitAtRisk.aspx",
            "Units%24ctl02%24item",
        )
        .respond_with(html(UNITS_2))
        .mount(a)
        .await;
        get("/CAND_AC_Apolices/PaymentReceiptList.aspx", RECEIPTS)
            .mount(a)
            .await;
        post(
            "/CAND_AC_Apolices/PaymentReceiptList.aspx",
            "Receipts%24ctl00%24Detail",
        )
        .respond_with(redirect("/CAND_AC_Apolices/ReceiptDetail.aspx?Guid=r1"))
        .mount(a)
        .await;
        get("/CAND_AC_Apolices/ReceiptDetail.aspx", RECEIPT_DETAIL)
            .mount(a)
            .await;
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
    assert_eq!(policies.len(), 3);
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
    assert_eq!(docs[0].date.to_string(), "2030-11-16");
    assert_eq!(docs[0].policy_number.as_deref(), Some("MR00000002"));
    assert_eq!(docs[0].id(), "MR00000002:2030-11-16:fatura-recibo:0");

    // A document that belongs to no policy.
    assert_eq!(docs[1].policy_number, None);
    assert_eq!(
        docs[1].id(),
        "-:2030-11-12:extrato-de-seguros-de-capitalizacao-e-ppr:0"
    );

    // Two identical entries stay distinguishable.
    assert_eq!(
        docs[2].id(),
        "0000000001:2030-10-12:aviso-recibo-de-premio:0"
    );
    assert_eq!(
        docs[3].id(),
        "0000000001:2030-10-12:aviso-recibo-de-premio:1"
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

fn day(y: i32, m: u32, d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(y, m, d).unwrap()
}

#[tokio::test]
async fn the_basic_list_costs_nothing_extra_and_the_detailed_one_adds_the_home_cards() {
    let servers = Servers::start().await;
    let client = servers.client().await;

    let basic = client.policies().await.unwrap();
    assert!(
        basic
            .iter()
            .all(|p| p.renewal_date.is_none() && p.agent.is_none())
    );

    let policies = client.policies_with_details().await.unwrap();
    // Matched by number, and with a receipt waiting to be paid.
    assert_eq!(policies[0].renewal_date, Some(day(2055, 4, 15)));
    assert_eq!(policies[0].agent.as_deref(), Some("agente exemplo sa"));
    assert_eq!(policies[0].payment_due, Some(day(2030, 4, 15)));
    assert_eq!(policies[0].risk_units, None);
    // The home insurance's card shows "2 unidade(s) risco(s)" instead of a number: matched by name.
    assert_eq!(policies[1].renewal_date, Some(day(2031, 3, 10)));
    assert_eq!(policies[1].risk_units, Some(2));
    assert_eq!(policies[1].payment_due, None);
    // Only visible after "Ver mais seguros".
    assert_eq!(policies[2].renewal_date, Some(day(2060, 7, 20)));
}

#[tokio::test]
async fn reads_the_general_data_of_a_policy_and_never_the_bank_details() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let policies = client.policies().await.unwrap();

    // A plain policy: the home page is consulted to find its card.
    let detail = client.policy_detail(&policies[1]).await.unwrap();
    assert_eq!(detail.started, Some(day(2030, 3, 10)));
    assert_eq!(detail.renews_or_ends, Some(day(2031, 3, 10)));
    let period = detail.period.unwrap();
    assert_eq!(
        (period.from, period.to),
        (day(2030, 3, 10), day(2031, 3, 10))
    );
    assert_eq!(detail.agent.as_deref(), Some("agente exemplo sa"));
    assert_eq!(detail.premium_cents, Some(12_345));
    assert_eq!(detail.payment_periodicity.as_deref(), Some("anual"));
    assert_eq!(detail.payment_method.as_deref(), Some("cobrança bancária"));

    // The page also shows an IBAN, the direct-debit authorization and the policyholder.
    let shown = format!("{detail:?} {}", serde_json::to_string(&detail).unwrap());
    for secret in ["PT50", "99999999999", "FICTICIO"] {
        assert!(!shown.contains(secret), "{secret} leaked");
    }
}

#[tokio::test]
async fn lists_the_coverages_of_every_unit_of_a_policy() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let policies = client.policies_with_details().await.unwrap();

    let units = client.coverages(&policies[1]).await.unwrap();
    assert_eq!(units.len(), 2);
    assert_eq!(units[0].name, "Fração 0000-000");
    assert_eq!(
        units[0].info("valor do capital seguro"),
        Some("50.000,00 €")
    );
    assert_eq!(units[0].info("Plano Seguro"), Some("fração"));
    let c = &units[0].coverages;
    assert_eq!(
        (c[0].name.as_str(), &c[0].capital, &c[0].deductible),
        ("Assistência", &Capital::Included, &None)
    );
    assert_eq!(c[1].capital, Capital::Cents(5_000_000));
    assert_eq!(c[2].capital, Capital::Cents(100_000));
    assert_eq!(c[2].deductible.as_deref(), Some("10 %"));

    // The second unit needs its own postback.
    assert_eq!(units[1].name, "Conteúdo 0000-000");
    assert_eq!(units[1].info("Plano Seguro"), Some("conteúdo"));
    assert_eq!(units[1].coverages[1].name, "Furto qualificado ou roubo");
    assert_eq!(units[1].coverages[1].capital, Capital::Cents(500_000));
}

#[tokio::test]
async fn a_life_policy_gives_one_unit_with_its_data_and_no_coverages() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    let policies = client.policies().await.unwrap();

    let units = client.coverages(&policies[2]).await.unwrap();
    assert_eq!(units.len(), 1);
    assert_eq!(units[0].name, "Seguro Vida Crédito Teste");
    assert_eq!(units[0].info("Valor do Saldo"), Some("1 000,00 €"));
    assert!(units[0].coverages.is_empty());
}

#[tokio::test]
async fn lists_the_receipts_due_and_reads_one_in_detail() {
    let servers = Servers::start().await;
    let client = servers.client().await;

    let receipts = client.receipts_due().await.unwrap();
    assert_eq!(receipts.len(), 2);
    let first = &receipts[0];
    assert_eq!(first.number, "00000000001");
    assert_eq!(first.amount_cents, 1234);
    assert_eq!(first.product, "Vida Ind. Capitalização");
    assert_eq!(first.policy_number.as_deref(), Some("0000000001"));
    assert_eq!(first.due_date, day(2030, 4, 15));
    assert_eq!(first.payment_method, "Débito Directo");
    assert_eq!(receipts[1].amount_cents, 123_456);
    assert_eq!(receipts[1].payment_method, "Multibanco");

    let detail = client.receipt(first).await.unwrap();
    assert_eq!(detail.issued, Some(day(2030, 2, 1)));
    assert_eq!(detail.due_date, Some(day(2030, 4, 15)));
    assert_eq!(detail.period.unwrap().to, day(2030, 4, 14));
    assert_eq!(detail.insurance_period.unwrap().to, day(2055, 4, 15));
    let mb = detail.multibanco.unwrap();
    assert_eq!(
        (mb.entity.as_str(), mb.reference.as_str(), mb.amount_cents),
        ("11111", "123456789", 1234)
    );
    // No IBAN or authorization number anywhere in what is returned.
    let shown = serde_json::to_string(&client.receipt(first).await.unwrap()).unwrap();
    assert!(!shown.contains("PT50") && !shown.contains("99999999999"));
}

#[tokio::test]
async fn the_portals_error_page_is_an_error_not_an_empty_answer() {
    let servers = Servers::start().await;
    let client = servers.client().await;
    // The portal answers a postback with its error page, with a 200, now and then.
    Mock::given(method("POST"))
        .and(path(HOME_PATH))
        .and(body_string_contains("Home%24more"))
        .respond_with(
            ResponseTemplate::new(302)
                .insert_header("Location", "/FID_B2C_PA_CW/Exceptions.ErrorPage.aspx"),
        )
        .with_priority(1)
        .mount(&servers.area)
        .await;
    Mock::given(method("GET"))
        .and(path("/FID_B2C_PA_CW/Exceptions.ErrorPage.aspx"))
        .respond_with(html("<html><body>Ocorreu um erro.</body></html>"))
        .mount(&servers.area)
        .await;

    let err = client.policies_with_details().await.unwrap_err();
    assert!(matches!(err, Error::UnexpectedResponse { .. }), "{err:?}");
    assert!(err.to_string().contains("error page"), "{err}");
}

#[tokio::test]
async fn a_documents_page_without_its_insurance_filter_is_not_an_empty_list() {
    let area = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path(DOCS))
        .respond_with(html(
            r#"<html><body><form id="WebForm1" action="DocumentosEntidade.aspx"><input type="hidden" name="__OSVSTATE" value="x"></form></body></html>"#,
        ))
        .mount(&area)
        .await;
    let state: SessionState = serde_json::from_value(serde_json::json!({
        "nif": "123456789", "authenticated_at": "2026-01-01T00:00:00Z", "cookies": [],
    }))
    .unwrap();
    let config = Config {
        base_url: area.uri().parse().unwrap(),
        ..Config::default()
    };
    let client = Client::from_session(state, config).unwrap();
    for result in [
        client.policies().await.map(|_| ()),
        client.documents(None).await.map(|_| ()),
    ] {
        let err = result.unwrap_err();
        assert!(matches!(err, Error::UnexpectedResponse { .. }), "{err:?}");
    }
}
