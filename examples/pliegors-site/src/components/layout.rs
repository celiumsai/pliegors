use super::{footer, header};
use crate::content::{Locale, ShellCopy};
use pliego_dom::{IntoView, View, el};

/// Options for the body-level PliegoRS site composition.
#[derive(Clone, Copy, Debug)]
pub struct BaseLayout<'a> {
    pub locale: Locale,
    pub pathname: &'a str,
    pub page_class: &'a str,
    pub show_footer: bool,
    pub shell: &'a ShellCopy,
}

impl<'a> BaseLayout<'a> {
    pub fn new(locale: Locale, pathname: &'a str, shell: &'a ShellCopy) -> Self {
        Self {
            locale,
            pathname,
            page_class: "",
            show_footer: true,
            shell,
        }
    }

    #[must_use]
    pub fn page_class(mut self, page_class: &'a str) -> Self {
        self.page_class = page_class;
        self
    }

    /// Compose skip navigation, header, main content, footer and live announcer.
    pub fn render(self, page: View) -> View {
        let mut main = el("main").id("main").child(page);
        if !self.page_class.is_empty() {
            main = main.class(self.page_class);
        }
        let mut body = vec![
            el("a")
                .class("skip-link")
                .attr("href", "#main")
                .child(self.shell.skip.text(self.locale))
                .into_view(),
            header(self.locale, self.pathname, self.shell),
            main.into_view(),
        ];
        if self.show_footer {
            body.push(footer(self.locale, self.shell));
        }
        body.push(
            el("div")
                .class("site-toast")
                .attr("role", "status")
                .attr("aria-live", "polite")
                .attr("aria-atomic", "true")
                .attr("data-toast", "")
                .into_view(),
        );
        View::Fragment(body)
    }
}
