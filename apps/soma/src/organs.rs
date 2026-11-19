//! The Cleveland Clinic's own organ illustrations, embedded.
//!
//! Every zoomable region of the body answers with its article's
//! illustration — fetched from the Cleveland Clinic health library at
//! build time and carried offline, with the article named beside it
//! (that is the citing layer: picture, title and URL travel together).
//!
//! Decoding is lazy and cached: a JPEG costs a few megabytes of memory
//! only after a person actually opens that organ, and only once.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use vieww::foundation::Image;

/// One embedded illustration: the organ it belongs to, the JPEG bytes,
/// and the article it was published with.
struct Art {
    key: &'static str,
    jpeg: &'static [u8],
    /// The article's title, for the caption.
    title: &'static str,
    /// The article's URL, for the citation.
    url: &'static str,
}

const ART: &[Art] = &[
    Art { key: "bladder", jpeg: include_bytes!("../assets/organs/bladder.jpg"), title: "Bladder: Anatomy, Location, Function & Related Conditions", url: "https://my.clevelandclinic.org/health/body/25010-bladder" },
    Art { key: "blood", jpeg: include_bytes!("../assets/organs/blood.jpg"), title: "Blood: What It Is & Function", url: "https://my.clevelandclinic.org/health/body/24836-blood" },
    Art { key: "bones", jpeg: include_bytes!("../assets/organs/bones.jpg"), title: "Human Skull Anatomy", url: "https://my.clevelandclinic.org/health/body/skull-anatomy" },
    Art { key: "brain", jpeg: include_bytes!("../assets/organs/brain.jpg"), title: "Brain: Parts, Function, How It Works & Conditions", url: "https://my.clevelandclinic.org/health/body/22638-brain" },
    Art { key: "ears", jpeg: include_bytes!("../assets/organs/ears.jpg"), title: "Ear: Anatomy, Facts & Function", url: "https://my.clevelandclinic.org/health/body/24048-ear" },
    Art { key: "eyes", jpeg: include_bytes!("../assets/organs/eyes.jpg"), title: "Eyes: How They Work, Anatomy & Common Conditions", url: "https://my.clevelandclinic.org/health/body/21823-eyes" },
    Art { key: "gallbladder", jpeg: include_bytes!("../assets/organs/gallbladder.jpg"), title: "Gallbladder: What It Is, Function, Location & Anatomy", url: "https://my.clevelandclinic.org/health/body/21690-gallbladder" },
    Art { key: "head", jpeg: include_bytes!("../assets/organs/head.jpg"), title: "Neck Muscles: What They Are, Types, Anatomy & Function", url: "https://my.clevelandclinic.org/health/body/21670-neck-muscles" },
    Art { key: "heart", jpeg: include_bytes!("../assets/organs/heart.jpg"), title: "Heart: Anatomy & Function", url: "https://my.clevelandclinic.org/health/body/21704-heart" },
    Art { key: "intestines", jpeg: include_bytes!("../assets/organs/intestines.jpg"), title: "Small Intestine: Function, Parts, Length & Location", url: "https://my.clevelandclinic.org/health/body/22135-small-intestine" },
    Art { key: "kidneys", jpeg: include_bytes!("../assets/organs/kidneys.jpg"), title: "Kidneys: Location, Anatomy, Function & Health", url: "https://my.clevelandclinic.org/health/body/21824-kidney" },
    Art { key: "liver", jpeg: include_bytes!("../assets/organs/liver.jpg"), title: "Liver: Where It’s Located, Function & Anatomy", url: "https://my.clevelandclinic.org/health/body/21481-liver" },
    Art { key: "lungs", jpeg: include_bytes!("../assets/organs/lungs.jpg"), title: "Lungs: Location, Anatomy, Function & Complications", url: "https://my.clevelandclinic.org/health/body/8960-lungs" },
    Art { key: "mouth", jpeg: include_bytes!("../assets/organs/mouth.jpg"), title: "Tongue: Definition, Location, Anatomy & Function", url: "https://my.clevelandclinic.org/health/body/22845-tongue" },
    Art { key: "muscles", jpeg: include_bytes!("../assets/organs/muscles.jpg"), title: "Muscles of the Body", url: "https://my.clevelandclinic.org/health/body/21887-muscle" },
    Art { key: "nerves", jpeg: include_bytes!("../assets/organs/nerves.jpg"), title: "Nervous System: What It Is, Parts, Function & Disorders", url: "https://my.clevelandclinic.org/health/body/21202-nervous-system" },
    Art { key: "pancreas", jpeg: include_bytes!("../assets/organs/pancreas.jpg"), title: "Pancreas: Function, Location, Anatomy & Living Without One", url: "https://my.clevelandclinic.org/health/body/21743-pancreas" },
    Art { key: "reproductive", jpeg: include_bytes!("../assets/organs/reproductive.jpg"), title: "Male Reproductive System: Structure & Function", url: "https://my.clevelandclinic.org/health/body/9117-male-reproductive-system" },
    Art { key: "skin", jpeg: include_bytes!("../assets/organs/skin.jpg"), title: "Skin: Layers, Structure and Function", url: "https://my.clevelandclinic.org/health/body/10978-skin" },
    Art { key: "spine", jpeg: include_bytes!("../assets/organs/spine.jpg"), title: "Spine: Anatomy, Function, Parts, Segments & Disorders", url: "https://my.clevelandclinic.org/health/body/10040-spine-structure-and-function" },
    Art { key: "spleen", jpeg: include_bytes!("../assets/organs/spleen.jpg"), title: "Spleen: Function, Location & Size, Possible Problems", url: "https://my.clevelandclinic.org/health/body/21567-spleen" },
    Art { key: "stomach", jpeg: include_bytes!("../assets/organs/stomach.jpg"), title: "Stomach: Anatomy, Function & Location", url: "https://my.clevelandclinic.org/health/body/21758-stomach" },
    Art { key: "thyroid", jpeg: include_bytes!("../assets/organs/thyroid.jpg"), title: "Thyroid: What It Is, Function & Problems", url: "https://my.clevelandclinic.org/health/body/23188-thyroid" },
];

thread_local! {
    /// The decode cache — one organ, one decode, ever.
    static CACHE: RefCell<HashMap<&'static str, Rc<Image>>> =
        RefCell::new(HashMap::new());
}

/// The organ's illustration, decoded on first ask.
///
/// Returns `None` when the organ has no embedded art (a few regions of the
/// body have none) — callers treat that as "text only" rather than an error.
#[must_use]
pub fn illustration(key: &str) -> Option<Rc<Image>> {
    let static_key = ART.iter().find(|a| a.key == key)?;
    CACHE.with(|cache| {
        if let Some(img) = cache.borrow().get(static_key.key) {
            return Some(Rc::clone(img));
        }
        let decoded = vieww_image::codec::decode(static_key.jpeg)
            .ok()
            .map(|d| d.into_image())?;
        let img = Rc::new(decoded);
        cache.borrow_mut().insert(static_key.key, Rc::clone(&img));
        Some(img)
    })
}

/// The article the organ's illustration was published with: `(title, url)`.
#[must_use]
pub fn credit(key: &str) -> Option<(&'static str, &'static str)> {
    ART.iter().find(|a| a.key == key).map(|a| (a.title, a.url))
}
