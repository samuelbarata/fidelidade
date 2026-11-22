//! Small parsers shared by the page scrapers: money, dates, and the portal's label/value rows.

use std::sync::LazyLock;

use chrono::NaiveDate;
use regex::Regex;
use scraper::{ElementRef, Html};

use crate::html::{selector, text_of};
use crate::model::slug;

/// `999.999,99 €` → `99999999` cents. `None` when it is not an amount (`Incluído`, ...).
pub(crate) fn parse_eur(text: &str) -> Option<i64> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"^(-?)(\d{1,3}(?:[. ]\d{3})*|\d+)(?:,(\d{1,2}))?$").unwrap());
    let t = text.trim().trim_end_matches('€').trim();
    let c = RE.captures(t)?;
    let whole: i64 = c[2].replace(['.', ' '], "").parse().ok()?;
    let frac = c.get(3).map_or(0, |f| {
        let n: i64 = f.as_str().parse().unwrap_or(0);
        if f.as_str().len() == 1 { n * 10 } else { n }
    });
    let cents = whole.checked_mul(100)?.checked_add(frac)?;
    Some(if &c[1] == "-" { -cents } else { cents })
}

/// `2030-03-16`.
pub(crate) fn parse_iso(text: &str) -> Option<NaiveDate> {
    NaiveDate::parse_from_str(text.trim(), "%Y-%m-%d").ok()
}

/// The first `2030-03-16` found in `text`.
pub(crate) fn find_iso(text: &str) -> Option<NaiveDate> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap());
    parse_iso(RE.find(text)?.as_str())
}

/// The first `27-12-2026` found in `text` (the home page's "Renovação a ...").
pub(crate) fn find_dmy(text: &str) -> Option<NaiveDate> {
    static RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\d{2}-\d{2}-\d{4}").unwrap());
    NaiveDate::parse_from_str(RE.find(text)?.as_str(), "%d-%m-%Y").ok()
}

/// `De 2026-01-01 a 2026-12-31` → both ends.
pub(crate) fn date_range(text: &str) -> Option<(NaiveDate, NaiveDate)> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r"(\d{4}-\d{2}-\d{2})\s*a\s*(\d{4}-\d{2}-\d{2})").unwrap());
    let c = RE.captures(text)?;
    Some((parse_iso(&c[1])?, parse_iso(&c[2])?))
}

/// The portal's `label / value` rows (`Covers_PolicyData`), in page order, with the labels as
/// the page shows them. Empty values are kept as empty strings; [`value`] looks a label up
/// without caring about accents or case.
pub(crate) fn rows(doc: &Html) -> Vec<(String, String)> {
    let row = selector(".Covers_PolicyData");
    let key = selector(".Covers__PolicyDataTitle");
    let value = selector(".Covers__PolicyDataText");
    doc.select(&row)
        .filter_map(|r| pair(r, &key, &value))
        .collect()
}

fn pair(
    row: ElementRef<'_>,
    key: &scraper::Selector,
    value: &scraper::Selector,
) -> Option<(String, String)> {
    let k = text_of(row.select(key).next()?);
    let v = row.select(value).next().map(text_of).unwrap_or_default();
    Some((k, v))
}

/// The first value of the row labelled `label` (any accents/case: `"Data Início"`).
pub(crate) fn value<'a>(rows: &'a [(String, String)], label: &str) -> Option<&'a str> {
    let wanted = slug(label);
    rows.iter()
        .find(|(k, v)| slug(k) == wanted && !v.is_empty())
        .map(|(_, v)| v.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn euros() {
        assert_eq!(parse_eur("999.999,99 €"), Some(99_999_999));
        assert_eq!(parse_eur("20,43 €"), Some(2043));
        assert_eq!(parse_eur("9.999,9 €"), Some(999_990));
        assert_eq!(parse_eur("12 €"), Some(1200));
        assert_eq!(parse_eur("1 234,50 €"), Some(123_450));
        assert_eq!(parse_eur("Incluído"), None);
        assert_eq!(parse_eur(""), None);
    }

    #[test]
    fn dates() {
        assert_eq!(
            find_iso("Pagamento até 2030-04-15 - Débito"),
            NaiveDate::from_ymd_opt(2030, 4, 15)
        );
        assert_eq!(
            find_dmy("Renovação a 15-04-2030"),
            NaiveDate::from_ymd_opt(2030, 4, 15)
        );
        assert_eq!(
            date_range("De 2026-01-01 a 2026-12-31"),
            Some((
                NaiveDate::from_ymd_opt(2026, 1, 1).unwrap(),
                NaiveDate::from_ymd_opt(2026, 12, 31).unwrap()
            ))
        );
        assert_eq!(find_iso("sem data"), None);
    }

    #[test]
    fn label_value_rows_ignore_accents_and_case() {
        let doc = Html::parse_fragment(
            r#"<div class="Covers_PolicyData"><div class="Covers__PolicyDataTitle">Data Renovação/Termo</div>
               <div class="Covers__PolicyDataText">2026-12-27</div></div>
               <div class="Covers_PolicyData"><div class="Covers__PolicyDataTitle">IBAN</div>
               <div class="Covers__PolicyDataText"></div></div>"#,
        );
        let rows = rows(&doc);
        assert_eq!(value(&rows, "data renovacao/termo"), Some("2026-12-27"));
        assert_eq!(value(&rows, "IBAN"), None, "empty values are not values");
    }
}
