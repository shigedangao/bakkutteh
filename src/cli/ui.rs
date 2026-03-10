use anyhow::{Result, anyhow};
use inquire::{
    Confirm, Select, Text, set_global_render_config,
    ui::{
        Attributes, Color, ErrorMessageRenderConfig, IndexPrefix, RenderConfig, StyleSheet, Styled,
    },
    validator::StringValidator,
};
use spinners::{Spinner, Spinners};
use std::fmt;

/// SpinnerWrapper is a wrapper around the spinners::Spinner struct
pub struct SpinnerWrapper(Spinner);

impl SpinnerWrapper {
    /// new creates a new SpinnerWrapper with the given message
    ///
    /// # Arguments
    ///
    /// * `msg` - S
    pub fn new<S: Into<String>>(msg: S) -> Self {
        Self(Spinner::new(Spinners::Dots9, msg.into()))
    }

    /// stop stops the spinner and prints a newline
    pub fn stop(&mut self) {
        self.0.stop_with_newline();
    }
}

/// Text implements a wrapper around the inquire's text component
///
/// # Arguments
///
/// * `title` - S
/// * `default_value` - Option<S>
pub fn text<S: AsRef<str>>(title: S, default_value: Option<S>) -> Result<String> {
    let mut text = Text::new(title.as_ref());
    if let Some(ref def) = default_value {
        text = text.with_default(def.as_ref());
    }

    match text.prompt() {
        Ok(res) => Ok(res),
        Err(err) => Err(anyhow!("Operation canceled: {:?}", err)),
    }
}

/// Text with validator add a validator to the text prompt
///
/// # Arguments
///
/// * `title` - S
/// * `validator` - F
pub fn text_with_validator<S: AsRef<str>, F: StringValidator>(
    title: S,
    validator: F,
) -> Result<String> {
    match Text::new(title.as_ref()).with_validator(validator).prompt() {
        Ok(res) => Ok(res),
        Err(err) => Err(anyhow!("Validation did not passed due to: {err}")),
    }
}

/// Select implements a wrapper around the inquire's select component
///
/// # Arguments
///
/// * `msg` - S
/// * `list` - Vec<S>
pub fn select<S: AsRef<str> + fmt::Display>(msg: S, list: Vec<S>) -> Result<S> {
    match Select::new(msg.as_ref(), list).prompt() {
        Ok(res) => Ok(res),
        Err(err) => Err(anyhow!("Unable to select the element due to: {err}")),
    }
}

/// Confirm implements a wrapper around the inquire's confirm component
///
/// # Arguments
///
/// * `msg` - S
/// * `default_value` - bool
pub fn confirm<S: AsRef<str>>(msg: S, default_value: bool) -> Result<bool> {
    Confirm::new(msg.as_ref())
        .with_default(default_value)
        .prompt()
        .map_err(|err| anyhow!("Unable to get the confirmation from the user: {err}"))
}

/// Initializes the Clack purple theme for the UI components. (done by Claude).
pub fn init_clack_purple_theme() {
    let mut config = RenderConfig::default();

    let bright = Color::rgb(237, 233, 254); // near-white purple tint — answers
    let muted = Color::rgb(148, 163, 184); // slate — secondary elements
    let lavender = Color::rgb(168, 85, 247); // electric violet — main accent
    let rose = Color::rgb(251, 113, 133); // errors only

    // ── Prompt state symbols ──
    config.prompt_prefix = Styled::new("◆").with_fg(lavender);
    config.answered_prompt_prefix = Styled::new("◇").with_fg(muted);
    config.canceled_prompt_indicator = Styled::new("◈  canceled").with_fg(muted);

    // ── Option navigation ──
    config.highlighted_option_prefix = Styled::new("❯").with_fg(lavender);
    config.unhighlighted_option_prefix = Styled::new(" ").with_fg(muted);
    config.scroll_up_prefix = Styled::new("  ↑").with_fg(muted);
    config.scroll_down_prefix = Styled::new("  ↓").with_fg(muted);

    // ── Checkboxes ──
    config.selected_checkbox = Styled::new("◼").with_fg(lavender);
    config.unselected_checkbox = Styled::new("◻").with_fg(muted);

    // ── Text styles ──
    config.answer = StyleSheet::new()
        .with_fg(bright)
        .with_attr(Attributes::BOLD);
    config.selected_option = Some(
        StyleSheet::new()
            .with_fg(bright)
            .with_attr(Attributes::BOLD),
    );
    config.help_message = StyleSheet::new()
        .with_fg(muted)
        .with_attr(Attributes::ITALIC);
    config.placeholder = StyleSheet::new().with_fg(muted);
    config.default_value = StyleSheet::new().with_fg(muted);

    // ── Indexing & errors ──
    config.option_index_prefix = IndexPrefix::None;
    config.error_message = ErrorMessageRenderConfig::default_colored()
        .with_prefix(Styled::new("▲").with_fg(rose))
        .with_message(StyleSheet::new().with_fg(rose));

    set_global_render_config(config);
}
