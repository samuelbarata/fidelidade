//! The customer area's home page: one card per policy, with what the documents page's
//! insurance filter does not know (renewal date, agent, a receipt waiting to be paid) and the
//! links to the policy's detail and coverage pages.

use std::collections::HashSet;
use std::sync::{Arc, LazyLock};

use chrono::NaiveDate;
use regex::Regex;
use scraper::{ElementRef, Html};

use crate::html::{self, Form, selector, text_of};
use crate::model::{Handle, Policy, slug};
use crate::{pages, parse};

#[derive(Debug, Default)]
pub(crate) struct Card {
    pub name: String,
    /// `Apólice nº ...`. Policies with several insured units show `2 unidade(s) risco(s)`
    /// instead, so this can be missing.
    pub number: Option<String>,
    pub risk_units: Option<u32>,
    pub renewal_date: Option<NaiveDate>,
    pub payment_due: Option<NaiveDate>,
    pub agent: Option<String>,
    /// `OsAjax` target of "Detalhe do seguro".
    pub detail: Option<String>,
    /// `__doPostBack` target of the "Coberturas" card.
    pub coverage: Option<String>,
}

/// The policy cards of the page.
pub(crate) fn cards(doc: &Html) -> Vec<Card> {
    static UNITS: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"(?i)(\d+)\s+unidade").unwrap());
    let title = selector(".insurance-home-list-item-title");
    let description = selector(".insurance-home-list-item-description");
    let sub = selector(".insurance-home-list-item-subdescription");
    let actions = selector(".insurance-home-list-item-actions a");
    let red_title = selector(".insurance-card-red-title");
    let red_description = selector(".insurance-card-red-description");
    let agent = selector(".insurance-card-agent-name");
    let anchors = selector("a[href*=__doPostBack]");

    doc.select(&selector(".insurance-home-list-item-card"))
        .map(|card| {
            let description = card
                .select(&description)
                .next()
                .map(text_of)
                .unwrap_or_default();
            let due = card
                .select(&red_title)
                .next()
                .filter(|t| slug(&text_of(*t)).contains("recibo"))
                .and_then(|_| card.select(&red_description).next())
                .and_then(|d| pages::parse_pt_date(&text_of(d)));
            Card {
                name: card.select(&title).next().map(text_of).unwrap_or_default(),
                number: pages::policy_number(&description),
                risk_units: UNITS.captures(&description).and_then(|c| c[1].parse().ok()),
                renewal_date: card
                    .select(&sub)
                    .next()
                    .and_then(|s| parse::find_dmy(&text_of(s))),
                payment_due: due,
                agent: card
                    .select(&agent)
                    .next()
                    .map(text_of)
                    .filter(|a| !a.is_empty()),
                detail: card
                    .select(&actions)
                    .next()
                    .and_then(|a| a.value().attr("onclick"))
                    .and_then(html::os_ajax_target),
                coverage: card
                    .select(&anchors)
                    .find(|a| slug(&text_of(*a)).starts_with("coberturas"))
                    .and_then(|a| a.value().attr("href"))
                    .and_then(html::do_postback_target),
            }
        })
        .collect()
}

/// The `OsAjax` target of "Ver mais seguros", when the page still hides some policies.
pub(crate) fn more_target(doc: &Html) -> Option<String> {
    doc.select(&selector("a[onclick*=OsAjax]"))
        .find(|a: &ElementRef<'_>| slug(&text_of(*a)) == "ver-mais-seguros")
        .and_then(|a| a.value().attr("onclick"))
        .and_then(html::os_ajax_target)
}

/// Copy what the cards know into the policies from the insurance filter. A card is matched by
/// policy number, or (policies with several insured units show none) by product name when
/// exactly one policy and one card are left with that name. Policies without a card keep
/// `None` in the extra fields.
pub(crate) fn enrich(policies: &mut [Policy], cards: &[Card], form: &Arc<Form>) {
    let mut used = HashSet::new();
    let mut matched: Vec<Option<usize>> = vec![None; policies.len()];

    for (i, p) in policies.iter().enumerate() {
        if let Some(c) = cards
            .iter()
            .position(|c| c.number.as_deref() == Some(p.number.as_str()))
        {
            used.insert(c);
            matched[i] = Some(c);
        }
    }
    for (i, p) in policies.iter().enumerate() {
        if matched[i].is_some() {
            continue;
        }
        let name = slug(&p.name);
        let same_policy = (0..policies.len())
            .filter(|&j| matched[j].is_none() && slug(&policies[j].name) == name)
            .count();
        let candidates: Vec<usize> = (0..cards.len())
            .filter(|c| {
                !used.contains(c) && cards[*c].number.is_none() && slug(&cards[*c].name) == name
            })
            .collect();
        if same_policy == 1 && candidates.len() == 1 {
            used.insert(candidates[0]);
            matched[i] = Some(candidates[0]);
        }
    }

    for (p, m) in policies.iter_mut().zip(matched) {
        let Some(card) = m.map(|c| &cards[c]) else {
            continue;
        };
        p.renewal_date = card.renewal_date;
        p.agent = card.agent.clone();
        p.risk_units = card.risk_units;
        p.payment_due = card.payment_due;
        let handle = |target: &Option<String>| {
            target.as_ref().map(|t| Handle {
                form: form.clone(),
                target: t.clone(),
            })
        };
        p.detail = handle(&card.detail);
        p.coverage = handle(&card.coverage);
    }
}
