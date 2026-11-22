use std::fmt;
use std::sync::Arc;

use chrono::NaiveDate;
use serde::Serialize;

use crate::html::Form;

/// "Click" on a control of a page: the form as the portal rendered it (with its
/// server-side state) plus the control's name.
///
/// The portal addresses things by position inside the page's state, not by a stable
/// id, so a handle is only meaningful for the session that produced it and for a
/// while: list again if a download answers [`Error::UnexpectedResponse`].
///
/// [`Error::UnexpectedResponse`]: crate::Error::UnexpectedResponse
#[derive(Clone)]
pub(crate) struct Handle {
    pub form: Arc<Form>,
    pub target: String,
}

impl fmt::Debug for Handle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Handle(<page state>)")
    }
}

/// An insurance policy ("seguro") of the logged-in user.
#[derive(Clone, Debug, Serialize)]
pub struct Policy {
    /// Product name as shown by the portal, e.g. `Fidelidade Casa`.
    pub name: String,
    /// Policy number (`Apólice nº`), the portal's own identifier for the policy.
    pub number: String,
    /// Product family derived from the portal's icon (`Vida`, `Casa`, ...), when present.
    pub category: Option<String>,
    /// Next renewal (`Renovação a ...` on the home page).
    pub renewal_date: Option<NaiveDate>,
    /// Name of the agent or distributor, as the home page shows it.
    pub agent: Option<String>,
    /// How many insured units the policy has (`2 unidade(s) risco(s)`), for policies that
    /// show it (home insurance, for example).
    pub risk_units: Option<u32>,
    /// Date a receipt of this policy has to be paid by, when the home page flags one.
    pub payment_due: Option<NaiveDate>,
    #[serde(skip)]
    pub(crate) handle: Handle,
    /// Where "Detalhe do seguro" and "Coberturas" lead, when the home page lists the policy.
    #[serde(skip)]
    pub(crate) detail: Option<Handle>,
    #[serde(skip)]
    pub(crate) coverage: Option<Handle>,
}

/// One entry of "Os Meus Documentos".
///
/// The portal gives documents no id of its own, so [`Document::id`] is derived from what
/// it shows: policy, date, kind and the position among entries that share all three.
/// It is stable between listings unless the portal removes documents, which makes it a
/// good key for "have I already fetched this?". A content hash is the airtight one.
#[derive(Clone, Debug, Serialize)]
pub struct Document {
    /// Document type as shown by the portal: `Fatura-Recibo`, `Aviso-Recibo de Prémio`,
    /// `Condições Gerais`, `Extrato de Seguros de Capitalização e PPR`, ...
    pub kind: String,
    /// The date shown next to the document (issue date).
    pub date: NaiveDate,
    /// Policy the document belongs to. `None` for documents that belong to none
    /// (for example capitalisation statements).
    pub policy_number: Option<String>,
    pub(crate) ordinal: u32,
    #[serde(skip)]
    pub(crate) handle: Handle,
}

impl Document {
    /// A key that identifies this document across listings, e.g.
    /// `0000000001:2030-11-16:fatura-recibo:0`.
    pub fn id(&self) -> String {
        format!(
            "{}:{}:{}:{}",
            self.policy_number.as_deref().unwrap_or("-"),
            self.date,
            slug(&self.kind),
            self.ordinal
        )
    }
}

/// A premium receipt waiting to be paid ("Recibos a pagamento").
#[derive(Clone, Debug, Serialize)]
pub struct Receipt {
    /// Receipt number.
    pub number: String,
    /// Amount in cents (`2043` = 20.43 €).
    pub amount_cents: i64,
    /// Product line as shown (`Vida Ind. Capitalização`).
    pub product: String,
    /// Policy the receipt belongs to.
    pub policy_number: Option<String>,
    /// Last day to pay.
    pub due_date: NaiveDate,
    /// How it is paid (`Débito Directo`, ...), as shown.
    pub payment_method: String,
    #[serde(skip)]
    pub(crate) handle: Handle,
}

/// The detail page of a [`Receipt`].
///
/// The page also shows the IBAN and the direct-debit authorization; this crate leaves them
/// out on purpose.
#[derive(Clone, Debug, Serialize)]
pub struct ReceiptDetail {
    /// When the receipt was issued.
    pub issued: Option<NaiveDate>,
    /// Last day to pay.
    pub due_date: Option<NaiveDate>,
    /// The period the receipt covers.
    pub period: Option<DateRange>,
    /// The period of the insurance contract the receipt belongs to.
    pub insurance_period: Option<DateRange>,
    /// The Multibanco reference, for paying by hand instead of by direct debit.
    pub multibanco: Option<Multibanco>,
}

/// A Multibanco payment reference.
#[derive(Clone, Debug, Serialize)]
pub struct Multibanco {
    /// Entity code (`11111`).
    pub entity: String,
    /// Reference, digits only (`123456789`).
    pub reference: String,
    /// Amount in cents.
    pub amount_cents: i64,
}

/// An inclusive range of days.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
pub struct DateRange {
    /// First day.
    pub from: NaiveDate,
    /// Last day.
    pub to: NaiveDate,
}

/// The "Dados gerais" tab of a policy.
///
/// The page also shows the IBAN, the direct-debit authorization and the policyholder's
/// details; this crate leaves them out on purpose.
#[derive(Clone, Debug, Serialize)]
pub struct PolicyDetail {
    /// `Data Início`.
    pub started: Option<NaiveDate>,
    /// `Data Renovação/Termo`: next renewal, or the end for fixed-term policies.
    pub renews_or_ends: Option<NaiveDate>,
    /// `Período Seguro`: the current insurance period.
    pub period: Option<DateRange>,
    /// The agent or distributor.
    pub agent: Option<String>,
    /// `Prémio Anual/Único` in cents.
    pub premium_cents: Option<i64>,
    /// `Periodicidade de Pagamento` (`anual`, `mensal`, ...), as shown.
    pub payment_periodicity: Option<String>,
    /// `Forma de Pagamento` (`cobrança bancária`, ...), as shown.
    pub payment_method: Option<String>,
}

/// One insured unit of a policy: a dwelling, its contents, a vehicle...
#[derive(Clone, Debug, Serialize)]
pub struct RiskUnit {
    /// How the portal labels it (`Fração 1234-567`, `Conteúdo 1234-567`).
    pub name: String,
    /// The unit's own data as `(label, value)` rows, in page order: plan, address,
    /// occupation, insured capital... The labels depend on the kind of insurance. The address
    /// of a home is personal data.
    pub info: Vec<(String, String)>,
    /// What the unit is covered for.
    pub coverages: Vec<Coverage>,
}

impl RiskUnit {
    /// The value of the info row called `label` (`"Valor do capital seguro"`), ignoring case
    /// and accents.
    pub fn info(&self, label: &str) -> Option<&str> {
        let wanted = slug(label);
        self.info
            .iter()
            .find(|(k, _)| slug(k) == wanted)
            .map(|(_, v)| v.as_str())
    }
}

/// One coverage ("cobertura") of a [`RiskUnit`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct Coverage {
    /// What is covered (`Inundações`).
    pub name: String,
    /// The insured capital.
    pub capital: Capital,
    /// The deductible ("franquia") as shown, when the coverage has one.
    pub deductible: Option<String>,
}

/// The capital of a [`Coverage`].
#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub enum Capital {
    /// `Incluído`: covered, with no separate capital.
    Included,
    /// An amount in cents.
    Cents(i64),
    /// Anything else, as shown.
    Other(String),
}

/// A downloaded file.
#[derive(Clone, Debug)]
pub struct Download {
    /// File name suggested by the portal (`Fatura-Recibo.pdf`). Not unique: name the
    /// file after [`Document::id`] or the document's fields instead.
    pub filename: String,
    /// `Content-Type` of the response. The portal says `application/octet-stream` for
    /// PDFs, so look at [`Download::is_pdf`] instead.
    pub content_type: String,
    /// The file's bytes.
    pub bytes: Vec<u8>,
}

impl Download {
    /// `true` if the content starts with the PDF magic number.
    pub fn is_pdf(&self) -> bool {
        self.bytes.starts_with(b"%PDF-")
    }
}

/// Lowercase, accent-free, dash-separated version of `s` (`Condições Gerais` →
/// `condicoes-gerais`).
pub(crate) fn slug(s: &str) -> String {
    let mut out = String::new();
    for c in s.chars().flat_map(char::to_lowercase) {
        let c = match c {
            'á' | 'à' | 'â' | 'ã' | 'ä' => 'a',
            'é' | 'è' | 'ê' | 'ë' => 'e',
            'í' | 'ì' | 'î' | 'ï' => 'i',
            'ó' | 'ò' | 'ô' | 'õ' | 'ö' => 'o',
            'ú' | 'ù' | 'û' | 'ü' => 'u',
            'ç' => 'c',
            c => c,
        };
        if c.is_ascii_alphanumeric() {
            out.push(c);
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    out.trim_end_matches('-').to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugs() {
        assert_eq!(slug("Condições Gerais"), "condicoes-gerais");
        assert_eq!(
            slug("Extrato de Seguros de Capitalização e PPR"),
            "extrato-de-seguros-de-capitalizacao-e-ppr"
        );
        assert_eq!(slug("Aviso-Recibo de Prémio"), "aviso-recibo-de-premio");
    }
}
