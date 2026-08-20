//! The Alterion wordmark, inlined so the binary needs no asset pipeline.
//!
//! Taken from the shared `alterion-logo.svg`, with the generated class names
//! namespaced so it can be drawn more than once.
//!
//! **Both the colour and the size are stated rather than asked for.** The mark
//! as exported carried its paint in a `<style>` block inside the SVG and asked
//! for `currentColor`. That needs a renderer to apply a stylesheet nested
//! inside an inline SVG and then resolve a keyword against the colour
//! inherited from the page. It carried no width or height either, only a
//! viewBox, leaving its size to the box around it.
//!
//! A browser does all three. A renderer is not obliged to do any of them, and
//! where it does not, the mark comes out in the default paint at its intrinsic
//! size: the wrong colour, and large enough to show through from behind a
//! dialog. Neither is worth depending on for a wordmark, so the colour arrives
//! as a literal and the size sits on the element.

/// Aspect ratio of the mark, for sizing.
pub const LOGO_VIEWBOX: (f64, f64) = (436.15, 78.03);

/// The wordmark, drawn at `width` pixels in `colour`.
///
/// The height follows from the aspect ratio, so a caller states one number and
/// cannot stretch it by getting the other wrong.
pub fn logo(width: f64, colour: &str) -> String {
    let (box_w, box_h) = LOGO_VIEWBOX;
    let height = width * box_h / box_w;
    format!(r###"<svg preserveAspectRatio="xMidYMid meet" id="aop-logo" width="{width}" height="{height}" xmlns="http://www.w3.org/2000/svg" viewBox="0 0 436.15 78.03"><g id="aop-logo-g" fill="none" stroke="{colour}" stroke-miterlimit="10" stroke-width="6"><g id="uuid-81d8af63-7c5e-4962-81bc-ef32222cf740"><polyline class="aop-l1" points="2.72 74.53 33.98 7.13 65.24 74.53"/><path class="aop-l1" d="M37.69,74.53l-8.54-20.38,8.54,20.38Z"/><line class="aop-l1" x1="87.96" y1="4.51" x2="87.96" y2="56.6"/><polyline class="aop-l1" points="87.96 64.15 87.96 72.68 122.18 72.68"/><line class="aop-l1" x1="126.52" y1="6.11" x2="158.94" y2="6.11"/><line class="aop-l1" x1="162.52" y1="6.11" x2="168.9" y2="6.11"/><line class="aop-l1" x1="147.2" y1="6.11" x2="147.2" y2="75.55"/><polyline class="aop-l1" points="213.41 7.86 183.53 7.86 183.53 49.48"/><line class="aop-l2" stroke-width="2" x1="191.7" y1="53.56" x2="180.64" y2="53.56"/><polyline class="aop-l1" points="183.61 52.56 183.61 73.8 216.02 73.8"/><path class="aop-l1" d="M224,32.55V7c1.3-.24,17.54-3,28.42,8.51,4.31,4.7,7,10.66,7.66,17,.54,4.66-.54,9.37-3.08,13.32-5.29,8.05-14.53,8.86-15.66,8.94h-17.34"/><line class="aop-l1" x1="262.46" y1="75.11" x2="244.61" y2="54.24"/><line class="aop-l1" x1="281.5" y1="6.79" x2="281.5" y2="74.87"/><path class="aop-l1" d="M300.73,26.53c0-3.74,8.86-18.65,29.28-18.55,19.81.09,30.88,13,31.83,30.47,1,18.48-11.19,35.23-32.17,35.23-6.63-.05-13.15-1.63-19.07-4.61"/><polyline class="aop-l1" points="393.32 76.79 393.32 11.43 403.03 28.28"/><polyline class="aop-l1" points="423.45 51.17 433.15 67.25 433.15 4.87"/></g></g></svg>"###)
}
