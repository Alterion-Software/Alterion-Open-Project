use dioxus_core::LaunchConfig;
use winit::window::WindowAttributes;

/// The configuration for the desktop application.
pub struct Config {
    pub(crate) window_attributes: WindowAttributes,
    /// Fonts to make available in addition to whatever the machine has.
    ///
    /// A program that names a font in its stylesheet is at the mercy of
    /// whether that font is installed, and the answer differs on every machine
    /// it runs on. Where the name matches nothing, what gets drawn is whatever
    /// the matcher reaches for, which on one machine was a shapes font that
    /// rendered Latin letters as Greek glyphs. Carrying the font means the text
    /// is the same everywhere, which matters beyond looks: text metrics decide
    /// column widths and where lines break.
    pub(crate) fonts: Vec<&'static [u8]>,
}

impl LaunchConfig for Config {}

impl Default for Config {
    fn default() -> Self {
        Self {
            window_attributes: WindowAttributes::default().with_title(
                dioxus_cli_config::app_title().unwrap_or_else(|| "Dioxus App".to_string()),
            ),
            fonts: Vec::new(),
        }
    }
}

impl Config {
    pub fn new() -> Self {
        Self::default()
    }

    /// Carry a font with the program rather than hoping for it.
    ///
    /// The bytes of a font file, which stay registered for the life of the
    /// process. Naming the same family first in the stylesheet is what then
    /// makes it the one that gets used.
    pub fn with_fonts(mut self, fonts: Vec<&'static [u8]>) -> Self {
        self.fonts = fonts;
        self
    }

    /// Set the configuration for the window.
    pub fn with_window_attributes(mut self, attrs: WindowAttributes) -> Self {
        // We need to do a swap because the window builder only takes itself as muy self
        self.window_attributes = attrs;
        self
    }
}
