//! The local health library — 11,043 Cleveland Clinic topics, embedded,
//! with the semantic and graph layers.
//!
//! # The format
//!
//! `assets/db/soma_db.txt` is five sections separated by a lone `\x1d` line:
//!
//! 1. **topics** — one per line, fields joined by `\x1f`:
//!    `kind | organ | title | description | url`. `kind` is a single letter
//!    (B body, S symptom, D disease, X diagnostic, T treatment, P procedure,
//!    G general, M medication); `organ` is an index into section 2 or `-1`.
//! 2. **organs** — one per line: `id | name | key | x,y,z | zoom | cc_url`.
//! 3. **bodies** — records joined by `\x1e`: `topic_index \x1f markdown`.
//!    (Markdown keeps its own newlines, so `\n` cannot be the record
//!    separator here.)
//! 4. **terms** — the vector layer, one line per term:
//!    `term \x1f idf \x1f idx:tf;idx:tf;…` — the postings list every topic
//!    whose title or description carries the term, with the term's
//!    weighted frequency. The section's first line is `#norms \x1f n,n,…`,
//!    each topic's TF-IDF vector length, so cosine similarity at query time
//!    is a table lookup and a multiply. Terms are sorted, so lookup is a
//!    binary search; the shared tokenizer lives in `search.rs`.
//! 5. **graph** — the anatomy graph, one line per organ:
//!    `organ_key \x1f neighbor:weight;…` — edges are corpus co-mentions
//!    (a topic assigned to organ A whose article text also names organ B),
//!    normalized per row, the top eight kept. `search.rs` walks it to pull
//!    in the topics of neighboring organs — "chest pain" finds the heart
//!    page first and then the lungs' and blood's, because the corpus says
//!    those are the heart's neighbors.
//!
//! # Why not JSON
//!
//! Eleven thousand records of JSON on a phone is a second of parsing before
//! the first frame, for a structure a linear scan reproduces in tens of
//! milliseconds. The separator format also lets every field stay a
//! `&'static str` slice into the embedded text — the database is zero-copy,
//! and the body texts (10 MB of them) are split out lazily, on the first
//! topic a person actually opens, so startup never pays for them.

use std::cell::RefCell;
use std::rc::Rc;

const RAW: &str = include_str!("../assets/db/soma_db.txt");

/// One searchable health topic.
#[derive(Debug, Clone, Copy)]
pub struct Topic {
    /// B/S/D/X/T/P/G/M — see the module doc.
    pub kind: char,
    /// Index into [`Db::organs`], or `None`.
    pub organ: Option<usize>,
    pub title: &'static str,
    pub description: &'static str,
    pub url: &'static str,
}

/// One zoomable body region.
#[derive(Debug, Clone, Copy)]
pub struct Organ {
    pub name: &'static str,
    /// Short key, stable across rebuilds (`"heart"`, `"lungs"`).
    pub key: &'static str,
    /// Where the organ sits in the 3D body (body units, y up, facing +z).
    pub anchor: [f32; 3],
    /// How close the camera flies in (multiplier; smaller = closer).
    pub zoom: f32,
    /// The Cleveland Clinic anatomy page for this organ, when one exists.
    pub url: &'static str,
}

/// One row of the term index: a word, its rarity, and where it lives.
#[derive(Debug)]
pub struct TermEntry {
    pub term: &'static str,
    /// Inverse document frequency — rare words carry more meaning.
    pub idf: f32,
    /// `(topic index, weighted term frequency)` for every topic that
    /// carries the term.
    pub postings: Vec<(u32, f32)>,
}

/// The semantic layer: sorted terms, their postings, and per-topic vector
/// lengths for cosine similarity in one multiply.
#[derive(Debug, Default)]
pub struct TermIndex {
    pub entries: Vec<TermEntry>,
    /// `norms[topic]` — the length of that topic's TF-IDF vector.
    pub norms: Vec<f32>,
}

impl TermIndex {
    /// Binary-search a term. The section is written sorted for exactly
    /// this call.
    #[must_use]
    pub fn find(&self, term: &str) -> Option<&TermEntry> {
        self.entries
            .binary_search_by(|e| e.term.cmp(term))
            .ok()
            .map(|i| &self.entries[i])
    }

    /// The terms sharing a prefix, for completion while a person types.
    /// At most `limit` of them; the index is sorted so a prefix is a range.
    #[must_use]
    pub fn prefix(&self, prefix: &str, limit: usize) -> Vec<&'static str> {
        if prefix.is_empty() {
            return Vec::new();
        }
        let start = match self.entries.binary_search_by(|e| {
            e.term[..prefix.len().min(e.term.len())].cmp(prefix).then(std::cmp::Ordering::Less)
        }) {
            Ok(i) | Err(i) => i,
        };
        self.entries
            .iter()
            .skip(start)
            .take_while(|e| e.term.starts_with(prefix))
            .take(limit)
            .map(|e| e.term)
            .collect()
    }
}

/// The anatomy graph: organ -> its corpus-measured neighbors.
#[derive(Debug, Default)]
pub struct OrganGraph {
    /// `edges[organ] = [(neighbor organ, weight in 0..=1)]`.
    pub edges: Vec<Vec<(usize, f32)>>,
}

impl Topic {
    /// The human name of a kind letter.
    #[must_use]
    pub fn kind_name(self) -> &'static str {
        kind_name(self.kind)
    }
}

/// The human name of a kind letter.
#[must_use]
pub fn kind_name(kind: char) -> &'static str {
    match kind {
        'B' => "Body",
        'S' => "Symptom",
        'D' => "Condition",
        'X' => "Test",
        'T' => "Treatment",
        'P' => "Procedure",
        'M' => "Medication",
        _ => "Article",
    }
}

/// The parsed database. One per process, behind an `Rc` the app state owns.
#[derive(Debug)]
pub struct Db {
    pub topics: Vec<Topic>,
    pub organs: Vec<Organ>,
    /// The semantic vector layer (section 4).
    pub terms: TermIndex,
    /// The anatomy graph (section 5).
    pub graph: OrganGraph,
    bodies: RefCell<Option<Vec<(usize, &'static str)>>>,
}

/// Parse the embedded text. Cheap: linear passes, no allocation beyond
/// the vectors — body texts stay untouched until the first open.
#[must_use]
pub fn parse() -> Db {
    let mut sections = RAW.split("\n\x1d\n");
    let topic_lines = sections.next().unwrap_or("");
    let organ_lines = sections.next().unwrap_or("");
    let _bodies_raw = sections.next().unwrap_or("");
    let term_lines = sections.next().unwrap_or("");
    let graph_lines = sections.next().unwrap_or("");
    // The body section is the one with `_bodies_raw`; a body legitimately
    // containing the separator cannot happen (the builder guarantees it).

    let mut topics = Vec::new();
    for line in topic_lines.lines() {
        if line.is_empty() {
            continue;
        }
        let mut f = line.split('\x1f');
        let (Some(kind), Some(organ), Some(title), desc, url) =
            (f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let Some(kind) = kind.chars().next() else { continue };
        let organ = organ.parse::<i16>().ok().and_then(|n| {
            if n >= 0 {
                Some(n as usize)
            } else {
                None
            }
        });
        topics.push(Topic {
            kind,
            organ,
            title,
            description: desc.unwrap_or(""),
            url: url.unwrap_or(""),
        });
    }

    let mut organs = Vec::new();
    for line in organ_lines.lines() {
        if line.is_empty() {
            continue;
        }
        let mut f = line.split('\x1f');
        let (Some(_id), Some(name), Some(key), Some(anchor), Some(zoom), url) =
            (f.next(), f.next(), f.next(), f.next(), f.next(), f.next())
        else {
            continue;
        };
        let mut a = anchor.split(',').filter_map(|n| n.parse::<f32>().ok());
        let (Some(x), Some(y), Some(z)) = (a.next(), a.next(), a.next()) else {
            continue;
        };
        organs.push(Organ {
            name,
            key,
            anchor: [x, y, z],
            zoom: zoom.parse::<f32>().unwrap_or(0.5),
            url: url.unwrap_or(""),
        });
    }

    // --- section 4: the term index -------------------------------------
    let mut terms = TermIndex::default();
    for line in term_lines.lines() {
        if line.is_empty() {
            continue;
        }
        if let Some(norms) = line.strip_prefix("#norms\x1f") {
            terms.norms = norms.split(',').filter_map(|n| n.parse::<f32>().ok()).collect();
            continue;
        }
        let mut f = line.split('\x1f');
        let (Some(term), Some(idf), Some(postings)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let postings = postings
            .split(';')
            .filter_map(|cell| {
                let (idx, tf) = cell.split_once(':')?;
                Some((idx.parse::<u32>().ok()?, tf.parse::<f32>().ok()?))
            })
            .collect();
        terms.entries.push(TermEntry {
            term,
            idf: idf.parse::<f32>().unwrap_or(0.0),
            postings,
        });
    }
    // The builder writes the section sorted; sorting again is idempotent
    // and makes the binary search safe even against a hand-edited file.
    terms.entries.sort_by(|a, b| a.term.cmp(b.term));

    // --- section 5: the anatomy graph ----------------------------------
    let mut graph = OrganGraph { edges: vec![Vec::new(); organs.len()] };
    for line in graph_lines.lines() {
        if line.is_empty() {
            continue;
        }
        let (key, cells) = match line.split_once('\x1f') {
            Some(kv) => kv,
            None => continue,
        };
        let Some(src) = organs.iter().position(|o| o.key == key) else {
            continue;
        };
        let edges = cells
            .split(';')
            .filter_map(|cell| {
                let (name, weight) = cell.split_once(':')?;
                let dst = organs.iter().position(|o| o.key == name)?;
                Some((dst, weight.parse::<f32>().unwrap_or(0.0)))
            })
            .collect();
        if src < graph.edges.len() {
            graph.edges[src] = edges;
        }
    }

    Db {
        topics,
        organs,
        terms,
        graph,
        bodies: RefCell::new(None),
    }
}

impl Db {
    /// The full article body of a topic, if the corpus carries one.
    ///
    /// The first call splits the 10 MB body section; every later call is a
    /// binary search. Done lazily because a person opens one topic, not all
    /// 6,992, and startup should not pay for the 6,991 they never will.
    #[must_use]
    pub fn body(&self, index: usize) -> Option<&'static str> {
        {
            let borrowed = self.bodies.borrow();
            if let Some(bodies) = borrowed.as_ref() {
                return bodies
                    .binary_search_by_key(&index, |(i, _)| *i)
                    .ok()
                    .map(|pos| bodies[pos].1);
            }
        }
        let mut sections = RAW.split("\n\x1d\n");
        let _ = sections.next();
        let _ = sections.next();
        let raw_bodies = sections.next().unwrap_or("");
        let mut bodies: Vec<(usize, &'static str)> = Vec::new();
        for record in raw_bodies.split('\x1e') {
            if record.is_empty() {
                continue;
            }
            let mut f = record.splitn(2, '\x1f');
            let (Some(idx), Some(text)) = (f.next(), f.next()) else {
                continue;
            };
            if let Ok(idx) = idx.parse::<usize>() {
                bodies.push((idx, text));
            }
        }
        let out = bodies
            .binary_search_by_key(&index, |(i, _)| *i)
            .ok()
            .map(|pos| bodies[pos].1);
        *self.bodies.borrow_mut() = Some(bodies);
        out
    }
}

/// A shared, lazily-built database handle.
#[must_use]
pub fn shared() -> Rc<Db> {
    thread_local! {
        static DB: Rc<Db> = Rc::new(parse());
    }
    DB.with(Rc::clone)
}
