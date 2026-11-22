//! "Recibos a pagamento": the premium receipts waiting to be paid.

use std::sync::{Arc, LazyLock};

use regex::Regex;
use scraper::{ElementRef, Html};

use crate::error::{Error, Result};
use crate::html::{self, Form, selector, text_of};
use crate::model::{DateRange, Handle, Multibanco, Receipt, ReceiptDetail};
use crate::{pages, parse};

/// The rows of the list: `Recibo Nº 123 - 20,43 €` / `Vida Ind. Capitalização - Apólice Nº
/// 0000000001` / `Pagamento até 2030-04-15 - Débito Directo`.
pub(crate) fn list(doc: &Html, form: &Arc<Form>) -> Result<Vec<Receipt>> {
    static TITLE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)recibo\s*n[ºo.]*\s*(\S+)\s*-\s*([\d.,\s]+)\s*€").unwrap()
    });
    static WHEN: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r"(?i)pagamento\s+at[ée]\s+(\d{4}-\d{2}-\d{2})\s*-?\s*(.*)").unwrap()
    });
    let title = selector(".document-left-structure-title");
    let description = selector(".document-left-structure-description");
    let sub = selector(".document-left-structure-subdescription");
    let link = selector("a[href*=__doPostBack]");
    let unexpected = |what: String| Error::unexpected("reading a receipt row", what);

    let mut out = Vec::new();
    for t in doc.select(&title) {
        let Some((row, link)) = t
            .ancestors()
            .filter_map(ElementRef::wrap)
            .find_map(|el| el.select(&link).next().map(|l| (el, l)))
        else {
            continue;
        };
        let Some(target) = link.value().attr("href").and_then(html::do_postback_target) else {
            continue;
        };
        let title = text_of(t);
        let c = TITLE
            .captures(&title)
            .ok_or_else(|| unexpected(format!("unknown title {title:?}")))?;
        let amount_cents = parse::parse_eur(&c[2])
            .ok_or_else(|| unexpected(format!("unknown amount {:?}", &c[2])))?;
        let description = row
            .select(&description)
            .next()
            .map(text_of)
            .unwrap_or_default();
        let (product, policy) = match description.split_once(" - ") {
            Some((product, policy)) => (product.to_owned(), pages::policy_number(policy)),
            None => (description.clone(), None),
        };
        let when = row.select(&sub).next().map(text_of).unwrap_or_default();
        let w = WHEN
            .captures(&when)
            .ok_or_else(|| unexpected(format!("unknown payment line {when:?}")))?;
        out.push(Receipt {
            number: c[1].to_owned(),
            amount_cents,
            product,
            policy_number: policy,
            due_date: parse::parse_iso(&w[1])
                .ok_or_else(|| unexpected(format!("unknown date {:?}", &w[1])))?,
            payment_method: w[2].trim().to_owned(),
            handle: Handle {
                form: form.clone(),
                target,
            },
        });
    }
    Ok(out)
}

/// The detail page. The IBAN and the direct-debit authorization on it are not read.
pub(crate) fn detail(doc: &Html) -> ReceiptDetail {
    let rows = parse::rows(doc);
    let range = |label: &str| {
        parse::value(&rows, label)
            .and_then(parse::date_range)
            .map(|(from, to)| DateRange { from, to })
    };
    let multibanco = match (
        parse::value(&rows, "Entidade"),
        parse::value(&rows, "Referência"),
        parse::value(&rows, "Montante").and_then(parse::parse_eur),
    ) {
        (Some(entity), Some(reference), Some(amount_cents)) => Some(Multibanco {
            entity: entity.trim().to_owned(),
            reference: reference.chars().filter(char::is_ascii_digit).collect(),
            amount_cents,
        }),
        _ => None,
    };
    ReceiptDetail {
        issued: parse::value(&rows, "Emitido em").and_then(parse::find_iso),
        due_date: parse::value(&rows, "Data limite de pagamento").and_then(parse::find_iso),
        period: range("Período do recibo"),
        insurance_period: range("Período do seguro"),
        multibanco,
    }
}
