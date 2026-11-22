//! A policy's own pages: "Dados gerais" and the coverages of its insured units.

use scraper::{ElementRef, Html};

use crate::html::{selector, text_of};
use crate::model::{Capital, Coverage, PolicyDetail, RiskUnit, slug};
use crate::parse;

/// The "Dados gerais" tab. Only the fields that are about the contract: the page also holds
/// the IBAN, the direct-debit authorization and the policyholder, which are not read.
pub(crate) fn detail(doc: &Html) -> PolicyDetail {
    let rows = parse::rows(doc);
    let text = |label: &str| parse::value(&rows, label).map(str::to_owned);
    PolicyDetail {
        started: parse::value(&rows, "Data Início").and_then(parse::find_iso),
        renews_or_ends: parse::value(&rows, "Data Renovação/Termo").and_then(parse::find_iso),
        period: parse::value(&rows, "Período Seguro")
            .and_then(parse::date_range)
            .map(|(from, to)| crate::model::DateRange { from, to }),
        agent: text("Agente"),
        premium_cents: parse::value(&rows, "Prémio Anual/Único").and_then(parse::parse_eur),
        payment_periodicity: text("Periodicidade de Pagamento"),
        payment_method: text("Forma de Pagamento"),
    }
}

/// One tab of the insured-unit selector (`Fração 1234-567`, `Conteúdo 1234-567`).
#[derive(Debug)]
pub(crate) struct UnitTab {
    pub name: String,
    /// `OsAjax` target that shows this unit.
    pub target: Option<String>,
    /// The one the page currently shows.
    pub active: bool,
}

pub(crate) fn unit_tabs(doc: &Html) -> Vec<UnitTab> {
    doc.select(&selector("a[id*=RiskUnits]"))
        .map(|a| UnitTab {
            name: text_of(a),
            target: a
                .value()
                .attr("onclick")
                .and_then(crate::html::os_ajax_target),
            active: a.ancestors().filter_map(ElementRef::wrap).take(3).any(|e| {
                let classes: Vec<_> = e.value().classes().collect();
                classes.contains(&"cars-tabs-Item") && classes.contains(&"active")
            }),
        })
        .collect()
}

/// What the page shows for the currently selected unit: its data rows and its coverages.
/// `None` when the page has neither (life policies use another layout).
pub(crate) fn unit(doc: &Html, name: &str) -> Option<RiskUnit> {
    let item = selector(".AccordionVertical_item");
    let cover = selector(".Covers_Cover");
    let cell = selector(".Covers__Cell");
    let mut info = Vec::new();
    let mut coverages = Vec::new();

    for section in doc.select(&item) {
        let rows = section.select(&cover).collect::<Vec<_>>();
        if rows.is_empty() {
            let fragment = Html::parse_fragment(&section.html());
            info.extend(parse::rows(&fragment));
            continue;
        }
        for row in rows {
            let cells: Vec<ElementRef<'_>> = row.select(&cell).collect();
            let Some(first) = cells.first() else { continue };
            let name = text_of(*first);
            if name.is_empty() {
                continue;
            }
            let capital = cells.get(1).map(|c| text_of(*c)).unwrap_or_default();
            let deductible = cells.get(2).map(|c| text_of(*c)).filter(|d| !d.is_empty());
            coverages.push(Coverage {
                name,
                capital: capital_of(&capital),
                deductible,
            });
        }
    }

    if info.is_empty() && coverages.is_empty() {
        return None;
    }
    Some(RiskUnit {
        name: name.to_owned(),
        info,
        coverages,
    })
}

fn capital_of(text: &str) -> Capital {
    if let Some(cents) = parse::parse_eur(text) {
        Capital::Cents(cents)
    } else if slug(text) == "incluido" {
        Capital::Included
    } else {
        Capital::Other(text.to_owned())
    }
}
