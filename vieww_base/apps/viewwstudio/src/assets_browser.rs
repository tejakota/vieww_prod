//! ST-15: the project's `assets/`, browsable and previewable in the studio.
//!
//! # Why this module exists
//!
//! The runtime half of the story was already true: [`WorkspaceBundle`]
//! resolves a guest's asset reads against the open project's `assets/` before
//! the studio's own, and a Say `an image "…"` reads from the folder its
//! `.say` file sits in. But every way of *seeing* any of that was a file
//! manager: the Explorer listed `assets/` as names, opening a PNG produced
//! the lossy-text view a binary file gets — mojibake with a "Not valid UTF-8"
//! apology — and a font was eight megabytes of question marks. A designer
//! dropping a PNG in could not see it without leaving the tool.
//!
//! So the studio now has an Assets view beside the Explorer, and image and
//! font files open as previews rather than text:
//!
//! * [`Listing`] walks `<root>/assets` (bounded, hidden files skipped) and
//!   decodes a thumbnail per image — the thing a browser is *for*.
//! * [`Studio::open_path`] routes image and font files to
//!   [`Studio::open_asset`], which decodes or parses them once and shows
//!   them in the editor's pane: an image at up to 1024 px with its real
//!   dimensions, format and size; a font as a card of the metadata a font
//!   actually carries (family, style, weight, glyph count).
//! * Rename and delete-to-trash need nothing new: the Explorer's context
//!   menu already wraps every file row, `assets/` included, in
//!   `file_tree::rename` / `delete_to_trash`.
//! * [`Studio::missing_image_references`] is the usage report: every
//!   `an image "…"` in a rendered Say buffer that does not resolve under
//!   the `.say` file's folder becomes a Problem at the line that wrote it —
//!   the same moment the preview would have shown a broken-image
//!   placeholder, said as a diagnostic instead of squinted at.
//!
//! # The two asset conventions, said out loud
//!
//! `assets/` is the **AssetBundle** convention: a Rust screen reaches it
//! through the platform's asset service. A Say `an image "photos/cat.png"`
//! is the other one — Say reads from the `.say` file's own folder, and
//! refuses `..`. An asset under `assets/` therefore cannot be referenced by
//! a Say screen in `src/screens/` without moving it, and
//! [`Studio::insert_say_image`] says so rather than inventing a third
//! convention.
//!
//! # Honest limits
//!
//! * Thumbnails and previews decode on the UI thread. Bounded — 200 entries
//!   listed, 128 thumbnails, ≤ 96 px each — but a multi-second stall on a
//!   pathological folder is possible, and the cap is the mitigation, not a
//!   cure. A worker thread is the fix when a real project needs it.
//! * A font preview shows metadata, not glyphs set in that face: rendering
//!   studio text in a project font would mean loading it into the live font
//!   store, where it would change every fallback in the window. The card
//!   says what the font *is*; it does not pretend to typeset it.
//!
//! [`WorkspaceBundle`]: crate::assets::WorkspaceBundle

use std::path::{Path, PathBuf};

use vieww_asset::{decode_sized, dimensions, ImageFormat};
use vieww_foundation::Image;

use crate::state::{Diagnostic, Severity, Studio};

/// The most entries one listing holds. A folder past this is stated as
/// truncated rather than silently cut.
const MAX_ENTRIES: usize = 200;

/// The most thumbnails one listing decodes. Thumbnails are the expensive
/// half of a scan; the entries past this are listed by name only.
const MAX_THUMBNAILS: usize = 128;

/// The longest edge of a sidebar thumbnail.
const THUMBNAIL: u32 = 96;

/// The longest edge of an opened image preview. Larger files are decoded
/// down and the header keeps the truth — see [`dimensions`].
const PREVIEW: u32 = 1024;

/// The deepest folder either walk descends into — the listing's and the
/// signature's, so a symlink loop costs a bounded number of stats rather
/// than the walk's patience.
const MAX_DEPTH: usize = 8;

/// What a file in `assets/` is, as far as the studio can preview it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// An encoded image the codec can decode.
    Image,
    /// A font face `ttf-parser` can read.
    Font,
}

impl Kind {
    /// From a file extension, lowercased by the caller.
    #[must_use]
    fn of_extension(ext: &str) -> Option<Self> {
        match ext {
            "png" | "jpg" | "jpeg" | "webp" | "gif" | "bmp" => Some(Self::Image),
            "ttf" | "otf" => Some(Self::Font),
            _ => None,
        }
    }

    /// The label the row and the preview header show.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Image => "Image",
            Self::Font => "Font",
        }
    }
}

/// Is `path` a file the asset browser can preview?
///
/// The one question [`Studio::open_path`] asks before routing a click away
/// from the text editor. Extension-based, like every tool's; the codec and
/// the font parser still get to disagree with it, and their errors are shown
/// when they do.
#[must_use]
pub fn previewable(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| Kind::of_extension(&ext.to_ascii_lowercase()).is_some())
}

/// One file in the listing.
#[derive(Debug, Clone)]
pub struct Entry {
    /// The path under `assets/`, `/`-separated — the form an AssetBundle
    /// read uses.
    pub relative: String,
    /// The file's name alone.
    pub name: String,
    /// What the studio can do with it.
    pub kind: Kind,
    /// The size on disk, in bytes.
    pub bytes: u64,
    /// The decoded thumbnail, images only, absent past `MAX_THUMBNAILS`
    /// or when decoding refused.
    pub thumbnail: Option<Image>,
}

/// What `assets/` holds, or the honest statement that there is none.
#[derive(Debug, Clone, Default)]
pub struct Listing {
    /// The entries, in bundle-path order — sorted, not `read_dir` order, so
    /// two rescans of an unchanged folder cannot reshuffle the rows.
    pub entries: Vec<Entry>,
    /// `assets/` held more than `MAX_ENTRIES` files.
    pub truncated: bool,
    /// The files, sizes and mtimes the listing was built from — the guard
    /// that keeps a watcher-driven rescan from re-decoding a folder that
    /// did not change. Same idea as `file_tree`'s `mtime_snapshot`.
    signature: Vec<(PathBuf, u64, std::time::SystemTime)>,
}

impl Listing {
    /// Walk `<root>/assets` and build the listing.
    ///
    /// `with_thumbnails` is the caller's choice: a scan for change
    /// detection passes `false` and reads only the signature; the scan that
    /// will be shown decodes. See the module docs for the cost and its cap.
    #[must_use]
    pub fn scan(root: Option<&Path>, with_thumbnails: bool) -> Self {
        let mut listing = Self {
            entries: Vec::new(),
            truncated: false,
            signature: Vec::new(),
        };
        let Some(root) = root else { return listing };
        let assets = root.join("assets");
        if !assets.is_dir() {
            return listing;
        }
        let mut thumbnails = 0_usize;
        walk(
            &assets,
            &assets,
            0,
            &mut listing,
            &mut thumbnails,
            with_thumbnails,
        );
        // Sorted by bundle path, because `read_dir` order is the file
        // system's whim (inode hash on ext4) and a browser whose rows jump
        // around between two rescans of the *same* folder reads as broken.
        listing.entries.sort_by(|a, b| a.relative.cmp(&b.relative));
        // The signature the change guard compares against, taken from the
        // walk `same_files_as` itself uses. A scan that recorded nothing
        // would re-decode every thumbnail on every watcher tick — the guard
        // was always false — and a scan that recorded its own truncated
        // view would never agree with the detector's. A second stat walk on
        // a scan that just decoded images is the cheap half of it.
        listing.signature = signature_of(Some(root));
        listing
    }

    /// `true` when the files on disk are the ones this listing was built
    /// from — the cheap half of a refresh, so a watcher tick over an
    /// unchanged folder costs a stat walk and no pixels.
    #[must_use]
    pub fn same_files_as(&self, root: Option<&Path>) -> bool {
        signature_of(root) == self.signature
    }

    /// The entry whose relative path is `relative`, if the listing holds it.
    #[must_use]
    pub fn find(&self, relative: &str) -> Option<&Entry> {
        self.entries.iter().find(|entry| entry.relative == relative)
    }
}

/// The mtime signature of `<root>/assets`: the same walk [`Listing::scan`]
/// does, with no decoding, so the two cannot disagree about what a folder
/// holds.
fn signature_of(root: Option<&Path>) -> Vec<(PathBuf, u64, std::time::SystemTime)> {
    let mut out = Vec::new();
    let Some(root) = root else { return out };
    let assets = root.join("assets");
    if !assets.is_dir() {
        return out;
    }
    walk_signature(&assets, 0, &mut out);
    out.sort();
    out
}

/// One directory of the listing walk: entries in read order, subfolders
/// descended into when met, hidden entries skipped, [`MAX_DEPTH`] respected
/// so a symlink loop costs a bounded number of stats here too — not only in
/// the signature's walk.
fn walk(
    assets: &Path,
    directory: &Path,
    depth: usize,
    listing: &mut Listing,
    thumbnails: &mut usize,
    with_thumbnails: bool,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in read.flatten() {
        if listing.entries.len() >= MAX_ENTRIES {
            listing.truncated = true;
            return;
        }
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk(
                assets,
                &path,
                depth + 1,
                listing,
                thumbnails,
                with_thumbnails,
            );
            continue;
        }
        let relative = path
            .strip_prefix(assets)
            .map(|rest| rest.to_string_lossy().replace('\\', "/"))
            .unwrap_or_else(|_| name.clone());
        let kind = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| Kind::of_extension(&ext.to_ascii_lowercase()))
            .unwrap_or(Kind::Image);
        let thumbnail = if with_thumbnails && kind == Kind::Image && *thumbnails < MAX_THUMBNAILS {
            // One read, one decode; a file that refuses either is listed
            // without a thumbnail rather than dropping the folder.
            std::fs::read(&path)
                .ok()
                .and_then(|bytes| decode_sized(&bytes, THUMBNAIL, THUMBNAIL).ok())
                .map(|(image, _)| {
                    *thumbnails += 1;
                    image
                })
        } else {
            None
        };
        listing.entries.push(Entry {
            relative,
            name,
            kind,
            bytes: meta.len(),
            thumbnail,
        });
    }
}

/// The signature walk, with the same skips as [`walk`] above.
fn walk_signature(
    directory: &Path,
    depth: usize,
    out: &mut Vec<(PathBuf, u64, std::time::SystemTime)>,
) {
    if depth > MAX_DEPTH {
        return;
    }
    let Ok(read) = std::fs::read_dir(directory) else {
        return;
    };
    for entry in read.flatten() {
        let path = entry.path();
        if entry.file_name().to_string_lossy().starts_with('.') {
            continue;
        }
        let Ok(meta) = entry.metadata() else { continue };
        if meta.is_dir() {
            walk_signature(&path, depth + 1, out);
            continue;
        }
        let Ok(modified) = meta.modified() else {
            continue;
        };
        out.push((path, meta.len(), modified));
        if out.len() > 4 * MAX_ENTRIES {
            // The guard a change detector needs: a pathological tree cannot
            // make the cheap half of a refresh cost more than the dear half.
            break;
        }
    }
}

/// What an opened asset shows.
#[derive(Debug, Clone)]
pub enum Preview {
    /// A decoded image, with the truth about the file beside it.
    Image {
        /// The pixels, scaled to at most `PREVIEW` on an edge.
        image: Image,
        /// What the codec said the bytes were — a `.png` that is really a
        /// JPEG is reported as the JPEG it is.
        format: ImageFormat,
        /// The size the *file* is, from its header, not the scaled copy.
        width: u32,
        height: u32,
    },
    /// A font, as the metadata its tables carry.
    Font(FontCard),
}

/// What a font file says about itself.
#[derive(Debug, Clone, PartialEq)]
pub struct FontCard {
    /// The family name, name ID 1 — what a font menu would list.
    pub family: String,
    /// The subfamily, name ID 2, when the face offered one.
    pub style: String,
    /// The weight class as a number: 400 is Regular, 700 is Bold.
    pub weight: u16,
    /// How many glyphs the face has.
    pub glyphs: u32,
    /// `true` when the face declared itself monospaced.
    pub monospaced: bool,
}

impl FontCard {
    /// Read the card out of a font file's bytes.
    ///
    /// A face with no readable name table still gets a card: the file name
    /// is the family and the style is unstated, because a preview that
    /// *refuses* over metadata nobody needs is a worse tool than one that
    /// shows what it has. `font_preview` is the layer
    /// that decides the bytes were not a font at all.
    #[must_use]
    pub fn read(bytes: &[u8], fallback_name: &str) -> Self {
        let face = ttf_parser::Face::parse(bytes, 0).ok();
        // `Names` iterates through `IntoIterator` — it is a lazy array with a
        // `get`, not an `Iterator` itself — and `to_string` is `Option<String>`
        // because a record's encoding can be one this build cannot decode.
        let name_of = |id: u16| -> Option<String> {
            face.as_ref()?
                .names()
                .into_iter()
                .find_map(|record| (record.name_id == id).then(|| record.to_string()).flatten())
        };
        let (weight, glyphs, monospaced) = face.as_ref().map_or((400, 0, false), |face| {
            (
                face.weight().to_number(),
                u32::from(face.number_of_glyphs()),
                face.is_monospaced(),
            )
        });
        Self {
            family: name_of(1).unwrap_or_else(|| fallback_name.to_owned()),
            style: name_of(2).unwrap_or_else(|| "—".to_owned()),
            weight,
            glyphs,
            monospaced,
        }
    }
}

/// An asset opened for preview: the path plus what the studio could do with
/// it.
#[derive(Debug, Clone)]
pub struct AssetView {
    /// The file on disk.
    pub path: PathBuf,
    /// Its name alone, for the pane's header.
    pub name: String,
    /// The preview, or the sentence that says why there is none.
    pub preview: Result<Preview, String>,
    /// The size on disk.
    pub bytes: u64,
}

impl AssetView {
    /// Read and preview `path`.
    ///
    /// One read, one decode: nothing is cached, because an open is a click,
    /// not a frame.
    fn open(path: &Path) -> Self {
        let name = path.file_name().map_or_else(
            || path.display().to_string(),
            |n| n.to_string_lossy().into_owned(),
        );
        let bytes = match std::fs::read(path) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Self {
                    path: path.to_path_buf(),
                    name,
                    preview: Err(format!("could not read {}: {error}", path.display())),
                    bytes: 0,
                };
            }
        };
        let len = bytes.len() as u64;
        let kind = path
            .extension()
            .and_then(|ext| ext.to_str())
            .and_then(|ext| Kind::of_extension(&ext.to_ascii_lowercase()))
            .unwrap_or(Kind::Image);
        let preview = match kind {
            Kind::Image => image_preview(&bytes),
            Kind::Font => font_preview(&bytes, &name),
        };
        Self {
            path: path.to_path_buf(),
            name,
            preview,
            bytes: len,
        }
    }
}

/// Decode an image for the preview pane: scaled pixels plus the header's
/// truth about the file's real size.
fn image_preview(bytes: &[u8]) -> Result<Preview, String> {
    let (image, format) =
        decode_sized(bytes, PREVIEW, PREVIEW).map_err(|error| error.to_string())?;
    let (width, height) = dimensions(bytes).unwrap_or((image.width(), image.height()));
    Ok(Preview::Image {
        image,
        format,
        width,
        height,
    })
}

/// Read a font for the preview pane.
fn font_preview(bytes: &[u8], name: &str) -> Result<Preview, String> {
    let card = FontCard::read(bytes, name);
    if ttf_parser::Face::parse(bytes, 0).is_err() {
        // Said as an error so the pane does not present a file-name guess
        // as a font's own family.
        return Err("not a font this build can read".to_owned());
    }
    Ok(Preview::Font(card))
}

impl Studio {
    /// Open `path` as an asset preview rather than a text buffer.
    ///
    /// The editor pane shows it in place of code until a buffer tab is
    /// clicked ([`Studio::focus_buffer`] clears it) or it is closed — the
    /// same rule a welcome pane follows: one thing in the pane, and the
    /// tabs say what the other things are.
    pub fn open_asset(&self, path: PathBuf) {
        let view = AssetView::open(&path);
        let note = match &view.preview {
            Ok(Preview::Image { width, height, .. }) => crate::i18n::tf(
                "{} \u{00d7} {} — the file's own size, from its header",
                &[&width.to_string(), &height.to_string()],
            ),
            Ok(Preview::Font(_)) => {
                crate::i18n::t("The font's metadata, as its own tables carry it.").to_owned()
            }
            Err(why) => why.clone(),
        };
        self.asset_view.set(Some(std::rc::Rc::new(view)));
        self.notify(&note);
    }

    /// Close the asset preview and hand the pane back to the active buffer.
    pub fn close_asset(&self) {
        self.asset_view.set(None);
    }

    /// The listing entry for `path`, when `path` is one.
    ///
    /// The preview pane's way back from an open preview to the row that
    /// opened it: the pane wants "insert this asset at the caret", which is
    /// an [`Entry`] method, and it holds only the path the row handed it.
    #[must_use]
    pub fn asset_entry(&self, path: &Path) -> Option<Entry> {
        let root = self.root.peek()?;
        let assets = root.join("assets");
        let relative = path.strip_prefix(&assets).ok()?;
        self.assets
            .get()
            .find(&relative.to_string_lossy().replace('\\', "/"))
            .cloned()
    }

    /// Bring the listing back in step with `assets/` on disk.
    ///
    /// Called from the same places `rescan_tree` is, and guarded by the
    /// listing's mtime signature: an unchanged folder costs a stat walk.
    pub fn refresh_assets(&self) {
        let root = self.root.peek().clone();
        if self.assets.get().same_files_as(root.as_deref()) {
            return;
        }
        let listing = Listing::scan(root.as_deref(), true);
        self.assets.set(std::rc::Rc::new(listing));
    }

    /// Every `an image "…"` in `source` that does not resolve under
    /// `say_dir`, as diagnostics at the lines that wrote them.
    ///
    /// The Say parse is the same one the canvas and inspector use
    /// (`parse_widget_lines`), so a half-edited file still gets its image
    /// lines checked — the check has to work on the file the user is typing
    /// in, not the one the compiler would accept.
    #[must_use]
    pub fn missing_image_references(
        &self,
        file: &str,
        source: &str,
        say_dir: Option<&Path>,
    ) -> Vec<Diagnostic> {
        vieww_say_codegen::line::parse_widget_lines(source)
            .into_iter()
            .filter(|line| line.kind == vieww_say_codegen::line::WidgetKindName::Image)
            .filter_map(|line| {
                let path = line.value.as_deref()?;
                let resolves = say_dir.is_some_and(|dir| dir.join(path).is_file());
                (!resolves).then(|| Diagnostic {
                    file: file.to_owned(),
                    severity: Severity::Warning,
                    code: "asset-missing".to_owned(),
                    message: format!(
                        "an image \u{201c}{path}\u{201d} is not under this file's folder — Say \
                         reads images from beside the .say file, and a path with .. is refused"
                    ),
                    help: None,
                    line: line.line,
                    column: 1,
                    end_line: line.line,
                    end_column: 1,
                })
            })
            .collect()
    }

    /// Insert `an image "…"` for `entry` into the active Say buffer.
    ///
    /// Inserted as a whole line after the caret's line, at that line's
    /// indent — the same whole-line rule snippets follow, because an asset
    /// dropped mid-identifier is the glued-together-line bug in a new coat.
    /// A caret at top level (the pragma a freshly opened file's caret sits
    /// on, a line between screens) anchors to the next line that opens a
    /// block instead: an image line at top level is not Say, and the insert
    /// refuses when no block opens below the caret.
    ///
    /// Refused when there is no Say buffer, and refused when the asset is
    /// not under the `.say` file's folder: Say reads images from beside the
    /// file and refuses `..`, so the honest choices are move the asset or
    /// insert nothing, and the notification says which.
    pub fn insert_say_image(&self, entry: &Entry) {
        let Some(buffer) = self.active() else {
            self.notify("There is no buffer to insert into.");
            return;
        };
        if buffer.language != crate::language::Language::Say {
            self.notify("Inserting an image needs a Say buffer — open the .say screen first.");
            return;
        }
        let Some(dir) = buffer.path.as_deref().and_then(Path::parent) else {
            self.notify("A scratch buffer has no folder for an image to live beside.");
            return;
        };
        let Some(root) = self.root.peek() else {
            self.notify("Open the project folder first — assets live in it.");
            return;
        };
        let asset_path = root.join("assets").join(&entry.relative);
        let relative = pathdiff(&asset_path, dir);
        if relative.split('/').any(|part| part == "..") {
            self.notify(&format!(
                "\u{201c}{}\u{201d} is not beside the .say file — Say reads images from its own \
                 folder and refuses .. , so move the asset there first",
                entry.relative
            ));
            return;
        }
        let text = buffer.value.text.clone();
        let caret = buffer.value.selection.extent.min(text.len());
        let mut line_start = text[..caret].rfind('\n').map_or(0, |i| i + 1);
        let mut line_end = text[line_start..]
            .find('\n')
            .map_or(text.len(), |i| line_start + i);
        // **A top-level caret anchors to the next block, not to itself.** A
        // freshly opened file's caret sits on the pragma at offset 0, and a
        // caret between screens sits on a top-level line; an `an image` at
        // top level is not Say at all — the parser only reads widgets
        // indented under a block opener. Walk forward to the first line that
        // opens one — the same `ends_with(':')` the child test below uses —
        // and become its child; if no block opens below the caret, say so
        // rather than writing a line that could never render.
        let mut line = &text[line_start..line_end];
        let top_level =
            !line.trim_end().ends_with(':') && line.trim_start_matches(' ').len() == line.len();
        if top_level {
            let mut cursor = line_end;
            let mut anchored = false;
            while cursor < text.len() {
                let start = cursor + 1;
                let end = text[start..].find('\n').map_or(text.len(), |i| start + i);
                if text[start..end].trim_end().ends_with(':') {
                    line_start = start;
                    line_end = end;
                    line = &text[line_start..line_end];
                    anchored = true;
                    break;
                }
                cursor = end;
            }
            if !anchored {
                self.notify(
                    "There is no screen below the caret to put the image in — put the caret \
                     inside one first.",
                );
                return;
            }
        }
        let indent = line.len() - line.trim_start_matches(' ').len();
        // A line that opens a block gets the child's indent; a sibling gets
        // its own. One lookahead, no parsing.
        let child = line.trim_end().ends_with(':');
        let insert_indent = indent + usize::from(child) * 4;
        let mut insertion = String::new();
        // `at` is the start of the line after the caret's — or the very end
        // when the caret's line is the file's last and has no newline, in
        // which case this supplies the one it lacks.
        let at = (line_end + 1).min(text.len());
        if at == line_end {
            insertion.push('\n');
        }
        insertion.push_str(&" ".repeat(insert_indent));
        insertion.push_str(&format!("an image \"{relative}\"\n"));
        let mut next = String::with_capacity(text.len() + insertion.len());
        next.push_str(&text[..at]);
        next.push_str(&insertion);
        next.push_str(&text[at..]);
        let caret_after = (at + insertion.trim_end_matches('\n').len() + 1).min(next.len());
        self.edit(vieww_foundation::TextEditingValue {
            text: next,
            selection: vieww_foundation::TextSelection::collapsed(caret_after),
            secondary: Vec::new(),
            composing: None,
        });
        self.notify(&format!("Inserted an image \u{201c}{relative}\u{201d}."));
    }
}

/// `path` relative to `base`, `/`-separated.
///
/// `std` has no relative-path helper; this is the shallow one the insert
/// needs — prefix-shared components only — and the caller refuses anything
/// that comes back with `..`, which is exactly the case Say refuses too.
fn pathdiff(path: &Path, base: &Path) -> String {
    let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let base = std::fs::canonicalize(base).unwrap_or_else(|_| base.to_path_buf());
    let mut path_parts = path.components().peekable();
    let mut base_parts = base.components().peekable();
    while path_parts
        .peek()
        .is_some_and(|part| base_parts.peek() == Some(part))
    {
        path_parts.next();
        base_parts.next();
    }
    let mut out: Vec<String> = Vec::new();
    for _ in base_parts {
        out.push("..".to_owned());
    }
    for part in path_parts {
        out.push(part.as_os_str().to_string_lossy().into_owned());
    }
    out.join("/")
}

/// `bytes` the way a person reads a file size: `812 B`, `4 KiB`, `1.4 MiB`.
///
/// Written here rather than reached for because the tree has no formatter of
/// its own, and a row that says `1048576` where its neighbours say `4 KiB` is
/// the kind of disagreement that gets "fixed" by a second formatter later.
/// Binary prefixes, because every tool that shows a file size in units of
/// 1000 is arguing with the file system about what a kilobyte is.
#[must_use]
pub fn human_bytes(bytes: u64) -> String {
    const UNIT: f64 = 1024.0;
    #[expect(
        clippy::cast_precision_loss,
        reason = "a byte count past 2^53 is not a file"
    )]
    let mut size = bytes as f64;
    let mut unit = "B";
    for next in ["KiB", "MiB", "GiB"] {
        if size < UNIT {
            break;
        }
        size /= UNIT;
        unit = next;
    }
    if unit == "B" {
        format!("{bytes} B")
    } else if size >= 10.0 {
        format!("{size:.0} {unit}")
    } else {
        format!("{size:.1} {unit}")
    }
}

/// The label a preview header shows for `format`.
///
/// `Debug` would do — `Png`, `Jpeg` — but it would also do it by accident:
/// nothing about `{:?}` promises a name a header can show, and `Other` is the
/// one case where a sentence beats a word.
#[must_use]
pub fn format_label(format: ImageFormat) -> &'static str {
    match format {
        ImageFormat::Png => "PNG",
        ImageFormat::Jpeg => "JPEG",
        ImageFormat::WebP => "WebP",
        ImageFormat::Gif => "GIF",
        ImageFormat::Bmp => "BMP",
        ImageFormat::Other => "decoded image",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A temp directory unique to this test.
    fn dir(tag: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "viewwstudio-assets-browser-{tag}-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::remove_dir_all(&path).ok();
        std::fs::create_dir_all(&path).expect("a temp directory");
        path
    }

    #[test]
    fn previewable_covers_the_codec_and_the_parser() {
        assert!(previewable(Path::new("assets/a.png")));
        assert!(previewable(Path::new("assets/b.TTF")));
        assert!(!previewable(Path::new("assets/c.rs")));
        assert!(!previewable(Path::new("assets/no-extension")));
    }

    #[test]
    fn a_listing_skips_hidden_files_and_reads_kinds() {
        let root = dir("kinds");
        let assets = root.join("assets");
        std::fs::create_dir_all(assets.join("photos")).unwrap();
        std::fs::write(assets.join("avatar.png"), b"not really a png").unwrap();
        std::fs::write(assets.join(".DS_Store"), b"junk").unwrap();
        std::fs::write(assets.join("photos/type.ttf"), b"junk").unwrap();
        let listing = Listing::scan(Some(&root), false);
        assert_eq!(listing.entries.len(), 2, "{:?}", listing.entries);
        assert_eq!(listing.entries[0].relative, "avatar.png");
        assert_eq!(listing.entries[0].kind, Kind::Image);
        assert_eq!(listing.entries[1].relative, "photos/type.ttf");
        assert_eq!(listing.entries[1].kind, Kind::Font);
        // And no thumbnail was decoded — the scan was told not to.
        assert!(listing.entries[0].thumbnail.is_none());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn the_signature_notices_a_change_and_ignores_a_stable_folder() {
        let root = dir("signature");
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        std::fs::write(assets.join("a.png"), b"one").unwrap();
        let listing = Listing::scan(Some(&root), false);
        assert!(listing.same_files_as(Some(&root)));
        // A different size is seen at once — no waiting for a timestamp.
        std::fs::write(assets.join("a.png"), b"a longer body").unwrap();
        assert!(!listing.same_files_as(Some(&root)));
        // And a same-size rewrite is seen once the filesystem's mtime tick
        // has passed: the signature is sizes *and* mtimes, and the tick is
        // this filesystem's honest resolution — a rewrite inside one tick
        // is invisible to every mtime watcher, this one included, which is
        // the tree's `mtime_snapshot` limit too.
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(assets.join("a.png"), b"two").unwrap();
        assert!(!listing.same_files_as(Some(&root)));
        // And a workspace without assets is a stable nothing.
        let empty = dir("empty");
        assert!(Listing::default().same_files_as(Some(&empty)));
        std::fs::remove_dir_all(&root).ok();
        std::fs::remove_dir_all(&empty).ok();
    }

    #[test]
    fn a_truncated_listing_says_so() {
        let root = dir("truncated");
        let assets = root.join("assets");
        std::fs::create_dir_all(&assets).unwrap();
        for index in 0..(MAX_ENTRIES + 5) {
            std::fs::write(assets.join(format!("{index:04}.png")), b"x").unwrap();
        }
        let listing = Listing::scan(Some(&root), false);
        assert!(listing.truncated);
        assert_eq!(listing.entries.len(), MAX_ENTRIES);
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn a_real_font_reads_its_own_card() {
        let bytes =
            include_bytes!("../../../crates/vieww-text/assets/DejaVuSansMono-Bold-subset.ttf");
        let card = FontCard::read(bytes, "fallback.ttf");
        assert!(card.family.contains("DejaVu Sans Mono"), "{card:?}");
        assert!(card.style.contains("Bold"), "{card:?}");
        assert_eq!(card.weight, 700);
        assert!(card.monospaced, "{card:?}");
        assert!(card.glyphs > 0);
        // And the preview agrees the bytes are a font.
        assert!(font_preview(bytes, "x.ttf").is_ok());
    }

    #[test]
    fn rubbish_bytes_are_refused_rather_than_guessed_at() {
        assert!(image_preview(b"not a png at all").is_err());
        assert!(font_preview(b"not a font at all", "x.ttf").is_err());
    }
}
