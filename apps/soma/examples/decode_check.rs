// Debug: can the framework's codecs decode every embedded organ JPEG?
fn main() {
    let db = soma::db::parse();
    let keys: Vec<&str> = db.organs.iter().map(|o| o.key).collect();
    for key in keys {
        match soma::organs::illustration(key) {
            Some(img) => println!("{key: <14} OK  {}x{}", img.width(), img.height()),
            None => println!("{key: <14} DECODE FAILED"),
        }
    }
    // The body base too.
    match vieww_image::codec::decode(include_bytes!("../assets/body/base.png")) {
        Ok(d) => println!("body-base      OK  (png)"),
        Err(e) => println!("body-base      PNG FAIL: {e:?}"),
    }
}
