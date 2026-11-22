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
    #[serde(skip)]
    pub(crate) handle: Handle,
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
    /// `0000000001:2026-11-16:fatura-recibo:0`.
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
