//! The page as static HTML, without a wasm toolchain.
//!
//! ```console
//! cargo run -p mytlabs --example snapshot -- <outdir>
//! ```
//!
//! Mounts `mytlabs::page` natively, walks it with `vieww-platform-web-dom`
//! exactly as the live mount does, serialises the result with `VNode::to_html`,
//! and writes `index.html` with the tree in place of the boot fallback — plus
//! the fonts it references. What it cannot include is the one thing only a
//! live page has: nothing, on this page — there is no canvas island, so the
//! snapshot *is* the page, statically.
//!
//! What it is for: screenshotting the page (in both colour schemes, by
//! emulating `prefers-color-scheme`), reviewing a change to the tree without
//! building wasm, and a page a crawler can read.

use std::path::PathBuf;

use vieww_element::ElementTree;
use vieww_platform_web_dom::tree::walk;

fn main() -> std::io::Result<()> {
    let out = PathBuf::from(
        std::env::args()
            .nth(1)
            .unwrap_or_else(|| "target/mytlabs-snapshot".into()),
    );
    let here = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    std::fs::create_dir_all(out.join("fonts"))?;

    vieww_platform_web_dom::set_color_tokens(mytlabs::COLOR_TOKENS);
    let mut tree = ElementTree::new();
    let root = tree.mount(mytlabs::page());
    let body: String = walk(&tree, root).iter().map(|n| n.to_html()).collect();

    // The splice is bounded by a marker rather than by the first `</div>`,
    // because this page's fallback has divs of its own.
    let page = std::fs::read_to_string(here.join("index.html"))?;
    let start = page.find("<div id=\"app\">").expect("index.html has #app");
    let end = page
        .find("<!-- app-end -->")
        .expect("index.html has the app-end marker");
    let mut html = String::with_capacity(page.len() + body.len());
    html.push_str(&page[..start]);
    html.push_str("<div id=\"app\">");
    html.push_str(&body);
    html.push_str("</div>\n");
    html.push_str(&page[end..]);
    // No wasm module here: the splash would wait for one forever.
    let html = html.replace(
        "<div id=\"mt-splash\"",
        "<div id=\"mt-splash\" class=\"done\"",
    );
    std::fs::write(out.join("index.html"), html)?;

    let assets = here.join("assets");
    for face in [
        "Geist-Regular",
        "Geist-Medium",
        "Geist-Bold",
        "GeistMono-Regular",
        "GeistMono-Medium",
    ] {
        let from = assets.join("fonts").join(format!("vw-{face}.ttf"));
        if from.exists() {
            std::fs::copy(&from, out.join("fonts").join(format!("{face}.ttf")))?;
        }
    }
    println!("{}", out.join("index.html").display());
    Ok(())
}
