//! HTML helpers for the portal's OutSystems pages.
//!
//! The customer area is a classic server-rendered OutSystems application: one big
//! `<form>` per page, with the page's server-side state in the hidden `__OSVSTATE`
//! field. "Clicking" something is posting that form back with `__EVENTTARGET` set to
//! the control's unique name, exactly like ASP.NET's `__doPostBack`. The pages also
//! support the same postbacks as plain (non-AJAX) requests and answer with the full
//! page, which is all this crate uses.

use std::sync::LazyLock;

use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use url::Url;

pub(crate) fn selector(s: &str) -> Selector {
    Selector::parse(s).expect("static selector")
}

pub(crate) fn parse(html: &str) -> Html {
    Html::parse_document(html)
}

pub(crate) fn collapse_whitespace(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

pub(crate) fn text_of(el: ElementRef<'_>) -> String {
    collapse_whitespace(&el.text().collect::<String>())
}

/// An HTML form as a browser would submit it.
#[derive(Debug, Clone)]
pub(crate) struct Form {
    pub action: Url,
    pub fields: Vec<(String, String)>,
}

impl Form {
    /// Set `name` to `value`, replacing the first existing field or appending.
    pub fn set(&mut self, name: &str, value: &str) {
        match self.fields.iter_mut().find(|(n, _)| n == name) {
            Some((_, v)) => *v = value.to_owned(),
            None => self.fields.push((name.to_owned(), value.to_owned())),
        }
    }

    /// The fields to send to "click" the control whose unique name is `target`.
    pub fn post_back(&self, target: &str) -> Vec<(String, String)> {
        let mut form = self.clone();
        form.set("__EVENTTARGET", target);
        form.set("__EVENTARGUMENT", "");
        form.fields
    }
}

/// The form with the given id (OutSystems pages use `WebForm1`).
pub(crate) fn find_form(doc: &Html, base: &Url, id: &str) -> Option<Form> {
    let form = doc.select(&selector(&format!("form#{id}"))).next()?;
    let action = form
        .value()
        .attr("action")
        .filter(|a| !a.trim().is_empty())
        .and_then(|a| base.join(a.trim()).ok())
        .unwrap_or_else(|| base.clone());
    let fields = form
        .select(&selector("input[name]"))
        .filter(|input| {
            let ty = input
                .value()
                .attr("type")
                .map(str::to_ascii_lowercase)
                .unwrap_or_default();
            match ty.as_str() {
                "submit" | "button" | "image" | "reset" | "file" => false,
                // Like a browser: unchecked boxes are not submitted.
                "checkbox" | "radio" => input.value().attr("checked").is_some(),
                _ => true,
            }
        })
        .filter_map(|input| {
            let name = input.value().attr("name")?.to_owned();
            let value = input.value().attr("value");
            let is_box = matches!(input.value().attr("type"), Some("checkbox" | "radio"));
            Some((
                name,
                value.unwrap_or(if is_box { "on" } else { "" }).to_owned(),
            ))
        })
        .collect();
    Some(Form { action, fields })
}

/// The unique control name (`FID$block$...`) OutSystems passes as the third argument of
/// an `OsAjax(event, 'clientId', 'uniqueName', ...)` handler. That name is what a plain
/// postback expects in `__EVENTTARGET`.
pub(crate) fn os_ajax_target(onclick: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"OsAjax\([^,]*,\s*'[^']*',\s*'([^']*)'"#).unwrap());
    Some(RE.captures(onclick)?.get(1)?.as_str().to_owned())
}

/// The control name in a `javascript:__doPostBack('name','')` link.
pub(crate) fn do_postback_target(href: &str) -> Option<String> {
    static RE: LazyLock<Regex> =
        LazyLock::new(|| Regex::new(r#"__doPostBack\('([^']*)'"#).unwrap());
    Some(RE.captures(href)?.get(1)?.as_str().to_owned())
}

/// Find a `window.location = "..."`-style redirect in a page body.
pub(crate) fn find_js_redirect(html: &str, base: &Url) -> Option<Url> {
    static RE: LazyLock<Regex> = LazyLock::new(|| {
        Regex::new(r#"(?:window\.)?location(?:\.href)?\s*=\s*['"]([^'"]+)['"]"#).unwrap()
    });
    let target = RE.captures(html)?.get(1)?.as_str();
    base.join(target).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn form_like_a_browser() {
        let html = r#"<form id="WebForm1" action="Other.aspx" method="post">
            <input type="hidden" name="__EVENTTARGET" value="">
            <input type="hidden" name="__OSVSTATE" value="abc">
            <input type="text" name="phone" value="">
            <input type="checkbox" name="remember">
            <input type="checkbox" name="agreed" checked>
            <input type="submit" name="go" value="Go">
        </form>"#;
        let base = Url::parse("https://x.example/Dir/Page.aspx").unwrap();
        let form = find_form(&parse(html), &base, "WebForm1").unwrap();
        assert_eq!(form.action.as_str(), "https://x.example/Dir/Other.aspx");
        let names: Vec<_> = form.fields.iter().map(|(n, _)| n.as_str()).collect();
        assert_eq!(names, ["__EVENTTARGET", "__OSVSTATE", "phone", "agreed"]);
        let sent = form.post_back("A$b");
        assert!(sent.contains(&("__EVENTTARGET".into(), "A$b".into())));
        assert!(sent.contains(&("__OSVSTATE".into(), "abc".into())));
    }

    #[test]
    fn extracts_postback_targets() {
        let onclick = "dataLayer.push({'event': 'x','eventLabel':'PPR'});OsAjax(arguments[0] || window.event,'A_b_c','A$b$c','','__OSVSTATE,',''); return false;";
        assert_eq!(os_ajax_target(onclick).as_deref(), Some("A$b$c"));
        assert_eq!(
            do_postback_target("javascript:__doPostBack('X$y$z','')").as_deref(),
            Some("X$y$z")
        );
    }
}
