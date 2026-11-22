//! Parsers for the "Os Meus Documentos" page.
//!
//! Matched against the real portal's markup (October-November 2026), tested on
//! synthetic pages. When the portal changes, look for `UnexpectedResponse`.

use std::collections::HashMap;
use std::sync::{Arc, LazyLock};

use chrono::NaiveDate;
use regex::Regex;
use scraper::{ElementRef, Html};

use crate::error::{Error, Result};
use crate::html::{self, Form, selector};
use crate::model::{Document, Handle, Policy};

/// The page's form, the way a browser would submit it.
pub(crate) fn form(doc: &Html, base: &url::Url) -> Result<Arc<Form>> {
    html::find_form(doc, base, "WebForm1")
        .map(Arc::new)
        .ok_or_else(|| Error::unexpected("reading the documents page", "no WebForm1 form"))
}

/// The entries of the "all insurances" dropdown.
pub(crate) fn policies(doc: &Html, form: &Arc<Form>) -> Vec<Policy> {
    static ICON: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"icon-([A-Za-z]+)").unwrap());
    let title = selector(".insurance-button-dropdown-item-title");
    let description = selector(".insurance-button-dropdown-item-description");
    let icon = selector(".insurance-button-dropdown-item-icon span");

    doc.select(&selector("a[onclick*=wtLRPoliciesCombo]"))
        .filter_map(|a| {
            let target = html::os_ajax_target(a.value().attr("onclick")?)?;
            let name = html::text_of(a.select(&title).next()?);
            let number = policy_number(&html::text_of(a.select(&description).next()?))?;
            let category = a
                .select(&icon)
                .next()
                .and_then(|span| span.value().attr("class"))
                .and_then(|class| ICON.captures(class))
                .map(|c| c[1].to_owned())
                // The portal prefixes generic icons with `icon-`; skip font-awesome's `fa-*`.
                .filter(|c| c != "fa");
            Some(Policy {
                name,
                number,
                category,
                handle: Handle {
                    form: form.clone(),
                    target,
                },
            })
        })
        .collect()
}

/// The document rows ("Fatura-Recibo / 16 de Novembro de 2026 / Apólice nº ...").
pub(crate) fn documents(doc: &Html, form: &Arc<Form>) -> Result<Vec<Document>> {
    let kind = selector(".inbox-left-structure-title");
    let date = selector(".inbox-left-structure-description");
    let policy = selector(".inbox-left-structure-subdescription");
    let link = selector("a[href*=__doPostBack]");

    let mut seen: HashMap<(Option<String>, NaiveDate, String), u32> = HashMap::new();
    let mut out = Vec::new();
    for title in doc.select(&kind) {
        // The row is the closest ancestor that also holds the "Ver documento" link.
        let Some((row, link)) = title
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find_map(|el| el.select(&link).next().map(|l| (el, l)))
        else {
            continue;
        };
        let Some(target) = link.value().attr("href").and_then(html::do_postback_target) else {
            continue;
        };
        let kind = html::text_of(title);
        let date_text = row
            .select(&date)
            .next()
            .map(html::text_of)
            .unwrap_or_default();
        let date = parse_pt_date(&date_text).ok_or_else(|| {
            Error::unexpected(
                "reading a document row",
                format!("unknown date {date_text:?}"),
            )
        })?;
        let policy_number = row
            .select(&policy)
            .next()
            .and_then(|el| policy_number(&html::text_of(el)));

        let n = seen
            .entry((policy_number.clone(), date, kind.clone()))
            .or_insert(0);
        let ordinal = *n;
        *n += 1;

        out.push(Document {
            kind,
            date,
            policy_number,
            ordinal,
            handle: Handle {
                form: form.clone(),
                target,
            },
        });
    }
    Ok(out)
}

/// `Apólice nº 0000000001` → `0000000001`.
fn policy_number(text: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)ap[óo]lice\s*n[ºo.]*\s*([A-Za-z0-9-]+)").unwrap());
    Some(RE.captures(text)?.get(1)?.as_str().to_owned())
}

/// `16 de Novembro de 2026` → 2026-11-16.
pub(crate) fn parse_pt_date(text: &str) -> Option<NaiveDate> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(?i)(\d{1,2})\s+de\s+(\p{L}+)\s+de\s+(\d{4})").unwrap());
    let c = RE.captures(text)?;
    let month = match c[2].to_lowercase().as_str() {
        "janeiro" => 1,
        "fevereiro" => 2,
        "março" | "marco" => 3,
        "abril" => 4,
        "maio" => 5,
        "junho" => 6,
        "julho" => 7,
        "agosto" => 8,
        "setembro" => 9,
        "outubro" => 10,
        "novembro" => 11,
        "dezembro" => 12,
        _ => return None,
    };
    NaiveDate::from_ymd_opt(c[3].parse().ok()?, month, c[1].parse().ok()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn portuguese_dates() {
        assert_eq!(
            parse_pt_date("16 de Novembro de 2026"),
            NaiveDate::from_ymd_opt(2026, 11, 16)
        );
        assert_eq!(
            parse_pt_date("1 de Março de 2025"),
            NaiveDate::from_ymd_opt(2025, 3, 1)
        );
        assert_eq!(parse_pt_date("31 de Fevereiro de 2025"), None);
        assert_eq!(parse_pt_date("hoje"), None);
    }

    #[test]
    fn policy_numbers() {
        assert_eq!(
            policy_number("Apólice nº 0000000001").as_deref(),
            Some("0000000001")
        );
        assert_eq!(
            policy_number("Apólice nº MR12345678").as_deref(),
            Some("MR12345678")
        );
        assert_eq!(policy_number("sem número"), None);
    }
}
