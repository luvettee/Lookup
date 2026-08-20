pub mod cdp;
pub mod chrome;
pub mod manager;
pub mod page;
pub mod render;
pub mod types;

pub use render::{
    extract_text, render_html, render_html_with_options, render_pdf, screenshot_png,
    screenshot_with_options, ImageFormat, RenderOptions, ScreenshotOptions,
};
