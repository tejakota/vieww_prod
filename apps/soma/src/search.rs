//! The search backend — a regex engine, a grep, a vector space and a
//! knowledge graph, in one file.
//!
//! # What the user asked for, and what that means here
//!
//! "The search must be semantic and graph search from the database — not
//! exact word search." Four layers answer that:
//!
//! * a **regex engine** — kept from the first round: a real one, written
//!   here rather than pulled from crates.io, with a step budget so a
//!   hostile pattern degrades instead of hanging.
//! * a **grep** — the plain-word path still scans titles and descriptions
//!   as substrings, because an exact title hit is the strongest signal
//!   there is and nothing should bury it.
//! * a **vector space** — every topic is a TF-IDF vector over its title
//!   and description (built into the database at build time, section 4 of
//!   `soma_db.txt`), and a query is scored by cosine similarity against
//!   it. This is the semantic layer: "chest pain" matches *Angina* not
//!   because the letters agree but because the words around them do.
//! * a **knowledge graph** — the corpus-measured anatomy graph (section
//!   5): once a leading hit names an organ, the topics of its anatomical
//!   neighbors get a weight-proportional boost, so asking about the heart
//!   also surfaces the lungs' and the blood's pages.
//!
//! Query understanding adds two small engines of its own:
//!
//! * **synonyms** — a curated consumer-to-clinical dictionary ("heart
//!   attack" also queries *myocardial infarction*), so the words people
//!   actually type reach the words the corpus actually uses.
//! * **completion** — the last token being typed is prefix-matched against
//!   the term index, so "migra" already scores against *migraine*.
//! * **typos** — a token that is *close* to an index term but not one
//!   ("migrane", "dyabetes", "kidny") joins the query at reduced weight,
//!   found by a bounded Damerau-Levenshtein scan over the term index. The
//!   clinical vocabulary is exactly the part of a health app people cannot
//!   spell, and a misspelling previously returned nothing at all.
//!
//! # Why no neural net
//!
//! A real embedding model is megabytes of weights and a hundred
//! milliseconds of inference on a phone — and the corpus is fixed, so the
//! vectors can be baked at build time instead. TF-IDF plus a curated
//! synonym dictionary answers "say it differently than the article does"
//! offline, instantly, in a few hundred kilobytes.

use crate::db::Db;

// ---------------------------------------------------------------------------
// The regex AST
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
enum Node {
    Char(char),
    Any,
    Class { negated: bool, items: Vec<ClassItem> },
    Start,
    End,
    WordBoundary,
    /// Kept for the engine's shape: a future `(?:…)` non-capturing group
    /// parses into this; plain `()` groups fold into the concat they wrap.
    #[allow(dead_code)]
    Group(Box<Node>),
    Concat(Vec<Node>),
    Alt(Vec<Node>),
    Repeat { node: Box<Node>, min: u32, max: Option<u32> },
}

#[derive(Debug, Clone)]
enum ClassItem {
    Single(char),
    Range(char, char),
    Digit,
    NotDigit,
    Word,
    NotWord,
    Space,
    NotSpace,
}

/// A compiled pattern.
#[derive(Debug, Clone)]
pub struct Regex {
    root: Node,
    /// Whether the whole pattern is anchored to the start (fast reject).
    anchored: bool,
}

#[derive(Debug)]
enum ParseErr {
    UnclosedGroup,
    UnclosedClass,
    BadRepeat,
    TrailingBackslash,
}

// ---------------------------------------------------------------------------
// Parsing
// ---------------------------------------------------------------------------

struct Parser {
    chars: Vec<char>,
    at: usize,
}

impl Parser {
    fn parse_alt(&mut self) -> Result<Node, ParseErr> {
        let mut branches = vec![self.parse_concat()?];
        while self.peek() == Some('|') {
            self.at += 1;
            branches.push(self.parse_concat()?);
        }
        Ok(if branches.len() == 1 {
            branches.pop().unwrap_or(Node::Any)
        } else {
            Node::Alt(branches)
        })
    }

    fn parse_concat(&mut self) -> Result<Node, ParseErr> {
        let mut items = Vec::new();
        while let Some(c) = self.peek() {
            if c == '|' || c == ')' {
                break;
            }
            self.at += 1;
            let atom = match c {
                '(' => {
                    let inner = self.parse_alt()?;
                    if self.peek() != Some(')') {
                        return Err(ParseErr::UnclosedGroup);
                    }
                    self.at += 1;
                    inner
                }
                '[' => self.parse_class()?,
                '.' => Node::Any,
                '^' => Node::Start,
                '$' => Node::End,
                '\\' => self.parse_escape()?,
                '*' | '+' | '?' => return Err(ParseErr::BadRepeat),
                '{' => {
                    // A literal unless what follows is the shape of a
                    // repeat: `{digits}`, `{digits,}` or `{digits,digits}`.
                    let looks_like_repeat = {
                        let mut i = self.at; // at the char after '{'
                        let mut digits = 0;
                        while self.chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
                            i += 1;
                            digits += 1;
                        }
                        if digits == 0 {
                            false
                        } else if self.chars.get(i) == Some(&',') {
                            i += 1;
                            while self.chars.get(i).is_some_and(|c| c.is_ascii_digit()) {
                                i += 1;
                            }
                            self.chars.get(i) == Some(&'}')
                        } else {
                            self.chars.get(i) == Some(&'}')
                        }
                    };
                    if looks_like_repeat {
                        return Err(ParseErr::BadRepeat);
                    }
                    Node::Char('{')
                }
                plain => Node::Char(plain),
            };
            items.push(self.parse_repeat(atom)?);
        }
        Ok(match items.len() {
            0 => Node::Concat(Vec::new()),
            1 => items.pop().unwrap_or(Node::Any),
            _ => Node::Concat(items),
        })
    }

    fn parse_repeat(&mut self, atom: Node) -> Result<Node, ParseErr> {
        let (min, max) = match self.peek() {
            Some('*') => {
                self.at += 1;
                (0, None)
            }
            Some('+') => {
                self.at += 1;
                (1, None)
            }
            Some('?') => {
                self.at += 1;
                (0, Some(1))
            }
            Some('{') => {
                let save = self.at;
                self.at += 1;
                let mut lo = String::new();
                while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                    lo.push(self.chars[self.at]);
                    self.at += 1;
                }
                if lo.is_empty() {
                    self.at = save;
                    return Ok(atom);
                }
                let lo: u32 = lo.parse().map_err(|_| ParseErr::BadRepeat)?;
                let hi = if self.peek() == Some(',') {
                    self.at += 1;
                    let mut hi = String::new();
                    while self.peek().is_some_and(|c| c.is_ascii_digit()) {
                        hi.push(self.chars[self.at]);
                        self.at += 1;
                    }
                    if hi.is_empty() {
                        None
                    } else {
                        Some(hi.parse().map_err(|_| ParseErr::BadRepeat)?)
                    }
                } else {
                    Some(lo)
                };
                if self.peek() != Some('}') {
                    self.at = save;
                    return Ok(atom);
                }
                self.at += 1;
                (lo, hi)
            }
            _ => return Ok(atom),
        };
        // A lazy `?` suffix is accepted and ignored: the engine reports
        // "is there a match", which greedy and lazy answer the same.
        if self.peek() == Some('?') {
            self.at += 1;
        }
        Ok(Node::Repeat {
            node: Box::new(atom),
            min,
            max,
        })
    }

    fn parse_class(&mut self) -> Result<Node, ParseErr> {
        let negated = self.peek() == Some('^');
        if negated {
            self.at += 1;
        }
        let mut items = Vec::new();
        let mut first = true;
        loop {
            let Some(c) = self.peek() else {
                return Err(ParseErr::UnclosedClass);
            };
            if c == ']' && !first {
                self.at += 1;
                break;
            }
            first = false;
            self.at += 1;
            let lo = if c == '\\' {
                match self.parse_escape_char()? {
                    ClassItem::Single(ch) => ch,
                    other => {
                        items.push(other);
                        continue;
                    }
                }
            } else {
                c
            };
            // Range?
            if self.peek() == Some('-') && self.chars.get(self.at + 1).is_some_and(|&c| c != ']')
            {
                self.at += 1;
                let hi_char = self.chars[self.at];
                self.at += 1;
                let hi = if hi_char == '\\' {
                    match self.parse_escape_char()? {
                        ClassItem::Single(ch) => ch,
                        _ => return Err(ParseErr::UnclosedClass),
                    }
                } else {
                    hi_char
                };
                items.push(ClassItem::Range(lo, hi));
            } else {
                items.push(ClassItem::Single(lo));
            }
        }
        Ok(Node::Class { negated, items })
    }

    fn parse_escape(&mut self) -> Result<Node, ParseErr> {
        let Some(c) = self.peek() else {
            return Err(ParseErr::TrailingBackslash);
        };
        self.at += 1;
        Ok(match c {
            'd' => Node::Class { negated: false, items: vec![ClassItem::Digit] },
            'D' => Node::Class { negated: false, items: vec![ClassItem::NotDigit] },
            'w' => Node::Class { negated: false, items: vec![ClassItem::Word] },
            'W' => Node::Class { negated: false, items: vec![ClassItem::NotWord] },
            's' => Node::Class { negated: false, items: vec![ClassItem::Space] },
            'S' => Node::Class { negated: false, items: vec![ClassItem::NotSpace] },
            'b' | 'B' => Node::WordBoundary,
            'n' => Node::Char('\n'),
            't' => Node::Char('\t'),
            'r' => Node::Char('\r'),
            other => Node::Char(other),
        })
    }

    fn parse_escape_char(&mut self) -> Result<ClassItem, ParseErr> {
        let Some(c) = self.peek() else {
            return Err(ParseErr::TrailingBackslash);
        };
        self.at += 1;
        Ok(match c {
            'd' => ClassItem::Digit,
            'D' => ClassItem::NotDigit,
            'w' => ClassItem::Word,
            'W' => ClassItem::NotWord,
            's' => ClassItem::Space,
            'S' => ClassItem::NotSpace,
            'b' | 'B' => ClassItem::Single('\u{0}'), // handled by caller
            'n' => ClassItem::Single('\n'),
            't' => ClassItem::Single('\t'),
            'r' => ClassItem::Single('\r'),
            other => ClassItem::Single(other),
        })
    }

    fn peek(&self) -> Option<char> {
        self.chars.get(self.at).copied()
    }
}

/// Compile a pattern. Case-insensitive by folding both sides through
/// `to_lowercase` at match time.
///
/// # Errors
///
/// A sentence naming what is wrong with the pattern.
pub fn compile(pattern: &str) -> Result<Regex, String> {
    let mut parser = Parser {
        chars: pattern.chars().collect(),
        at: 0,
    };
    let root = parser
        .parse_alt()
        .map_err(|e| format!("cannot parse that pattern: {e:?}"))?;
    if parser.at != parser.chars.len() {
        return Err("cannot parse that pattern: stray ')'".to_owned());
    }
    let anchored = matches!(&root, Node::Concat(items) if matches!(items.first(), Some(Node::Start)))
        || matches!(root, Node::Start);
    Ok(Regex { root, anchored })
}

/// Whether a query should be treated as a regex at all: it must carry a
/// metacharacter, and must not be *only* metacharacters (a lone `.` is more
/// likely a sentence end than a wildcard).
#[must_use]
pub fn looks_like_regex(query: &str) -> bool {
    let mut metas = 0;
    let mut alnum = 0;
    for c in query.chars() {
        match c {
            '.' | '*' | '+' | '?' | '|' | '(' | ')' | '[' | ']' | '{' | '}' | '^' | '$' => {
                metas += 1;
            }
            c if c.is_alphanumeric() => alnum += 1,
            _ => {}
        }
    }
    metas > 0 && alnum > 0
}

// ---------------------------------------------------------------------------
// Matching
// ---------------------------------------------------------------------------

const STEP_BUDGET: u32 = 40_000;

struct Matcher<'a> {
    text: &'a [char],
    steps: std::cell::Cell<u32>,
}

impl<'a> Matcher<'a> {
    fn class_matches(items: &[ClassItem], negated: bool, c: char) -> bool {
        let hit = items.iter().any(|item| match item {
            ClassItem::Single(s) => s.eq_ignore_ascii_case(&c) || s.to_lowercase().eq(c.to_lowercase()),
            ClassItem::Range(lo, hi) => {
                (lo.to_lowercase().next().unwrap_or(*lo)..=hi.to_lowercase().next().unwrap_or(*hi))
                    .contains(&c.to_lowercase().next().unwrap_or(c))
            }
            ClassItem::Digit => c.is_ascii_digit(),
            ClassItem::NotDigit => !c.is_ascii_digit(),
            ClassItem::Word => c.is_alphanumeric() || c == '_',
            ClassItem::NotWord => !(c.is_alphanumeric() || c == '_'),
            ClassItem::Space => c.is_whitespace(),
            ClassItem::NotSpace => !c.is_whitespace(),
        });
        hit != negated
    }

    fn is_word(c: char) -> bool {
        c.is_alphanumeric() || c == '_'
    }

    /// Does `node` match starting at `pos`? Returns the end position on
    /// success. Greedy with backtracking, bounded by the step budget.
    fn m(&self, node: &Node, pos: usize, k: &mut dyn FnMut(usize) -> bool) -> bool {
        let steps = self.steps.get();
        if steps == 0 {
            return false;
        }
        self.steps.set(steps - 1);
        match node {
            Node::Char(c) => {
                self.text
                    .get(pos)
                    .is_some_and(|t| t.eq_ignore_ascii_case(c) || t.to_lowercase().eq(c.to_lowercase()))
                    && k(pos + 1)
            }
            Node::Any => self.text.get(pos).is_some() && k(pos + 1),
            Node::Class { negated, items } => self
                .text
                .get(pos)
                .is_some_and(|t| Self::class_matches(items, *negated, *t))
                && k(pos + 1),
            Node::Start => pos == 0 && k(pos),
            Node::End => pos == self.text.len() && k(pos),
            Node::WordBoundary => {
                let before = pos > 0 && Self::is_word(self.text[pos - 1]);
                let after = self.text.get(pos).is_some_and(|c| Self::is_word(*c));
                (before != after) && k(pos)
            }
            Node::Group(inner) => self.m(inner, pos, k),
            Node::Concat(items) => self.concat(items, 0, pos, k),
            Node::Alt(branches) => branches.iter().any(|b| self.m(b, pos, k)),
            Node::Repeat { node, min, max } => self.repeat(node, *min, *max, pos, 0, k),
        }
    }

    fn concat(
        &self,
        items: &[Node],
        at: usize,
        pos: usize,
        k: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        match items.split_first() {
            None => k(pos),
            Some((head, rest)) => self.m(head, pos, &mut |next| self.concat(rest, at + 1, next, k)),
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn repeat(
        &self,
        node: &Node,
        min: u32,
        max: Option<u32>,
        pos: usize,
        count: u32,
        k: &mut dyn FnMut(usize) -> bool,
    ) -> bool {
        // Greedy: try one more repetition (when allowed), then give up and
        // let the continuation run — but never below `min` repetitions.
        let can_more = max.is_none_or(|m| count < m);
        if can_more
            && self.m(node, pos, &mut |next| {
                // Guard against zero-width repetition loops.
                next != pos && self.repeat(node, min, max, next, count + 1, k)
            })
        {
            return true;
        }
        count >= min && k(pos)
    }

    /// Is there a match anywhere in the text?
    fn search(&self, node: &Node) -> bool {
        let mut start = 0;
        loop {
            let s = start;
            if self.m(node, s, &mut |_| true) {
                return true;
            }
            if s >= self.text.len() {
                return false;
            }
            start = s + 1;
        }
    }
}

impl Regex {
    /// Case-insensitive containment test.
    #[must_use]
    pub fn is_match(&self, text: &str) -> bool {
        let chars: Vec<char> = text.chars().collect();
        let matcher = Matcher {
            text: &chars,
            steps: STEP_BUDGET.into(),
        };
        if self.anchored {
            return matcher.m(&self.root, 0, &mut |_| true);
        }
        matcher.search(&self.root)
    }
}

// ---------------------------------------------------------------------------
// Tokenization — shared with the database builder
// ---------------------------------------------------------------------------

/// The words that carry no meaning for similarity, and so are not indexed.
const STOP: &[&str] = &[
    "a", "an", "and", "are", "as", "at", "be", "but", "by", "can", "do", "does", "for",
    "from", "had", "has", "have", "how", "i", "if", "in", "into", "is", "it", "its", "may",
    "of", "on", "or", "should", "so", "than", "that", "the", "their", "them", "there",
    "they", "this", "to", "was", "we", "were", "what", "when", "where", "which", "who",
    "why", "will", "with", "you", "your", "not", "no", "also", "more", "most", "other",
    "some", "such", "then", "these", "those", "via", "about", "after", "all", "any",
    "been", "before", "being", "both", "could", "each", "few", "having", "he", "her",
    "his", "him", "me", "my", "our", "out", "over", "same", "she", "too", "under", "up",
    "down", "very", "while", "within", "without", "cause", "causes", "called", "common",
    "commonly", "like", "make", "makes", "made", "people", "person", "see", "seen",
    "symptom", "symptoms", "test", "tests", "tested", "treatment", "treatments", "care",
    "health", "condition", "conditions", "disease", "diseases", "learn", "know", "find",
    "get", "gets", "getting", "need", "needs", "used", "using", "type", "types", "ways",
    "way", "including", "include", "includes", "well", "many", "much", "even", "just",
    "often", "usually",
];

/// The same suffix stemmer the builder applies, in the same order. A
/// mirrored pair — change one and the vectors stop agreeing with the
/// queries, so both carry the other's test vectors.
///
/// The `es` rules are sibilant-aware: `es` is a real suffix only after
/// `ch`/`sh`/`x`/`z`/`ss` (watches, dishes, boxes, buzzes, classes), and
/// the whole `-aches` family is `-ache` + `s` (a headache plural is not a
/// watch plural). Everywhere else the `e` belongs to the word — `stones`
/// stems to `stone`, not `ston` — because the singular and the plural must
/// meet in one key, or a query for one can never find the other's page.
fn stem(word: &str) -> &str {
    if word.len() > 4 && word.ends_with("ies") {
        let stem = &word[..word.len() - 3];
        // "studies" -> "study", joined back by the caller.
        return stem;
    }
    if word.len() > 5 && word.ends_with("ing") {
        return &word[..word.len() - 3];
    }
    if word.len() > 4 && word.ends_with("ed") {
        return &word[..word.len() - 2];
    }
    if word.len() > 4 && word.ends_with("aches") {
        // headaches, backaches, earaches — "-ache" + "s".
        return &word[..word.len() - 1];
    }
    if word.len() > 4
        && (word.ends_with("sses") || word.ends_with("ches") || word.ends_with("shes"))
    {
        // classes, watches, dishes.
        return &word[..word.len() - 2];
    }
    if word.len() > 3
        && word.ends_with("es")
        && matches!(word.as_bytes().get(word.len() - 3), Some(b'x') | Some(b'z'))
    {
        // boxes, buzzes.
        return &word[..word.len() - 2];
    }
    if word.len() > 3 && word.ends_with('s') && !word.ends_with("ss") {
        return &word[..word.len() - 1];
    }
    word
}

/// Query text -> weighted search terms.
///
/// Lowercase, alphanumeric runs, stopwords out, suffixes stemmed — the
/// exact pipeline `build_db_semantic.py` ran over every title and
/// description, so a query lands in the same space the corpus lives in.
#[must_use]
pub fn tokenize(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in text.chars().chain([' ']) {
        if ch.is_ascii_alphanumeric() {
            current.push(ch.to_ascii_lowercase());
        } else if !current.is_empty() {
            push_term(&mut out, &current);
            current.clear();
        }
    }
    out
}

/// One raw word through the stoplist and the stemmer, with the
/// "ies"-rule's `y` reattached (the builder's python does `t[:-3] + "y"`).
fn push_term(out: &mut Vec<String>, raw: &str) {
    if raw.len() < 2 || STOP.contains(&raw) {
        return;
    }
    if raw.len() > 4 && raw.ends_with("ies") {
        let mut word = raw[..raw.len() - 3].to_owned();
        word.push('y');
        if !STOP.contains(&word.as_str()) {
            out.push(word);
        }
        return;
    }
    let stemmed = stem(raw);
    if stemmed.len() >= 2 && !STOP.contains(&stemmed) {
        out.push(stemmed.to_owned());
    }
}

// ---------------------------------------------------------------------------
// Synonyms — the consumer-to-clinical dictionary
// ---------------------------------------------------------------------------

/// (phrase, expansion): when the phrase appears in the query, the
/// expansion's tokens join it at 0.75 weight. The left side is how people
/// say it; the right side is how the corpus spells it.
const SYNONYMS: &[(&str, &str)] = &[
    ("heart attack", "myocardial infarction heart"),
    ("high blood pressure", "hypertension blood pressure"),
    ("low blood pressure", "hypotension blood pressure"),
    ("bad cholesterol", "ldl cholesterol lipid"),
    ("good cholesterol", "hdl cholesterol lipid"),
    ("stomach ache", "abdominal pain stomach"),
    ("tummy ache", "abdominal pain stomach"),
    ("belly pain", "abdominal pain stomach"),
    ("throwing up", "vomiting nausea"),
    ("throw up", "vomiting nausea"),
    ("feeling sick", "nausea vomiting"),
    ("kidney stone", "renal calculus kidney stone"),
    ("kidney stones", "renal calculus kidney stone"),
    ("pink eye", "conjunctivitis eye"),
    ("runny nose", "rhinorrhea nasal congestion"),
    ("stuffy nose", "nasal congestion nose"),
    ("blocked nose", "nasal congestion nose"),
    ("sore throat", "pharyngitis throat pain"),
    ("earache", "ear pain otalgia"),
    ("ear ache", "ear pain otalgia"),
    ("dizzy spells", "vertigo dizziness balance"),
    ("feeling dizzy", "vertigo dizziness balance"),
    ("short of breath", "dyspnea breathless breathing"),
    ("out of breath", "dyspnea breathless breathing"),
    ("cant sleep", "insomnia sleep"),
    ("trouble sleeping", "insomnia sleep"),
    ("headache", "cephalgia head pain"),
    ("chest pain", "angina chest discomfort"),
    ("feeling tired", "fatigue exhaustion energy"),
    ("tired", "fatigue exhaustion energy"),
    ("no energy", "fatigue exhaustion energy"),
    ("sugar diabetes", "diabetes glucose insulin"),
    ("blood sugar", "glucose diabetes insulin"),
    ("peeing a lot", "urination frequent urine"),
    ("blood in pee", "hematuria urine blood"),
    ("blood in stool", "hematochezia stool rectal bleeding"),
    ("the runs", "diarrhea loose stool"),
    ("cant poop", "constipation stool bowel"),
    ("bloody nose", "epistaxis nose bleed"),
    ("nose bleed", "epistaxis nose bleeding"),
    ("broken bone", "fracture bone break"),
    ("twisted ankle", "ankle sprain ligament"),
    ("bug bite", "insect bite sting"),
    ("bee sting", "insect sting venom"),
    ("allergic reaction", "allergy anaphylaxis histamine"),
    ("spots on skin", "acne pimple skin"),
    ("pimples", "acne comedone skin"),
    ("hair loss", "alopecia balding hair"),
    ("loud snoring", "snoring sleep apnea"),
    ("feeling nervous", "anxiety nervous worry"),
    ("panic attack", "anxiety panic"),
    ("feeling sad", "depression mood sadness"),
    ("mood swings", "mood bipolar"),
    ("memory loss", "dementia amnesia memory"),
    ("passed out", "syncope fainting"),
    ("swollen glands", "lymph node swelling"),
    ("swollen feet", "edema swelling fluid"),
    ("water retention", "edema swelling fluid"),
    ("the flu", "influenza flu virus"),
    ("common cold", "cold rhinovirus upper respiratory"),
    ("sun burn", "sunburn ultraviolet skin burn"),
    // No leading space: a phrase must fire wherever it sits in the query,
    // including at the very start, where " razor burn" never matched.
    ("razor burn", "razor bumps skin shaving"),
    ("acid reflux", "gerd gastroesophageal reflux heartburn"),
    ("heartburn", "gerd reflux acid"),
    ("winded", "breathless dyspnea"),
    ("womb", "uterus uterine"),
    ("belly", "abdomen abdominal"),
    ("pee", "urine urination"),
    ("poop", "stool bowel defecation"),
    ("farting", "flatulence gas"),
    ("period pain", "menstrual cramp dysmenorrhea"),
    ("heavy period", "menorrhagia menstrual bleeding"),
    ("missed period", "amenorrhea menstrual"),
    ("family planning", "contraception birth control"),
    ("weight loss", "weight loss bariatric"),
    ("obese", "obesity weight bariatric"),
    ("joint pain", "arthralgia joint arthritis"),
    ("muscle pain", "myalgia muscle"),
    ("back pain", "backache lumbago back"),
    ("neck pain", "cervical neck stiffness"),
    ("eye pain", "ophthalmalgia eye"),
    ("red eye", "conjunctivitis eye redness"),
    ("dry eye", "keratoconjunctivitis dry eye tear"),
    ("lazy eye", "amblyopia eye"),
    ("double vision", "diplopia vision"),
    ("blurry vision", "blurred vision refraction"),
    ("ringing in ears", "tinnitus ear"),
    ("hearing loss", "deafness hearing ear"),
    ("lockjaw", "tetanus trismus"),
    ("cold sore", "herpes labialis cold sore"),
    ("canker sore", "aphthous ulcer mouth"),
    ("mouth ulcer", "aphthous stomatitis mouth"),
    ("gum disease", "periodontitis gingivitis gum"),
    ("tooth decay", "dental caries cavity tooth"),
    ("wisdom teeth", "third molar wisdom tooth"),
    ("grinding teeth", "bruxism teeth jaw"),
    ("thyroid problem", "thyroid endocrine"),
    ("hormone problem", "endocrine hormone"),
    ("adrenal", "adrenal endocrine"),
    ("growth", "endocrine growth hormone"),
    ("smell loss", "anosmia smell"),
    ("loss of taste", "ageusia taste"),
    ("numbness", "paresthesia numb tingling"),
    ("pins and needles", "paresthesia tingling numb"),
    ("shingles", "herpes zoster shingles"),
    ("fungal infection", "mycosis fungal ringworm"),
    ("yeast infection", "candidiasis yeast"),
    ("urinary infection", "uti cystitis urinary"),
    ("bladder infection", "cystitis uti bladder"),
    ("kidney infection", "pyelonephritis kidney"),
    ("stomach bug", "gastroenteritis stomach virus"),
    ("food poisoning", "gastroenteritis foodborne"),
    ("fatty liver", "hepatic steatosis liver fat"),
    ("cirrhosis", "liver cirrhosis fibrosis"),
    ("yellow skin", "jaundice bilirubin skin"),
    ("yellow eyes", "jaundice icterus"),
    ("anemia", "anemia hemoglobin blood"),
    ("low iron", "iron deficiency anemia"),
    ("blood clot", "thrombosis clot embolism"),
    ("clot", "thrombosis embolism clot"),
    ("leakage", "incontinence leakage"),
    ("wet the bed", "enuresis bedwetting"),
    ("snoring", "snore apnea sleep"),
    ("sleepy", "somnolence sleepiness hypersomnia"),
    ("cant focus", "attention concentration adhd"),
    ("hyper", "adhd hyperactivity attention"),
    ("seizure", "epilepsy convulsion seizure"),
    ("fits", "seizure epilepsy convulsion"),
    ("stroke", "stroke cerebrovascular attack"),
    ("mini stroke", "tia transient ischemic attack"),
    ("dementia", "dementia alzheimer memory"),
    ("als", "amyotrophic lateral sclerosis"),
    ("ms", "multiple sclerosis"),
];

// ---------------------------------------------------------------------------
// Typos — the near-miss layer
// ---------------------------------------------------------------------------

/// Damerau-Levenshtein distance (optimal string alignment), or `None` the
/// moment the distance is known to exceed `limit`. Two rows and the row
/// above them, so a transposition — "teh" vs "the" — costs one, not two.
fn damerau_within(a: &[char], b: &[char], limit: usize) -> Option<usize> {
    let n = a.len();
    let m = b.len();
    if n.abs_diff(m) > limit {
        return None;
    }
    let mut prev2: Vec<usize> = Vec::new();
    let mut prev: Vec<usize> = (0..=m).collect();
    let mut row = vec![0_usize; m + 1];
    for i in 1..=n {
        row[0] = i;
        let mut row_min = row[0];
        for j in 1..=m {
            let cost = usize::from(a[i - 1] != b[j - 1]);
            // Substitute, delete from a, insert into a.
            let mut cell = (prev[j - 1] + cost).min(prev[j] + 1).min(row[j - 1] + 1);
            if i > 1 && j > 1 && a[i - 1] == b[j - 2] && a[i - 2] == b[j - 1] {
                cell = cell.min(prev2[j - 2] + 1);
            }
            row[j] = cell;
            row_min = row_min.min(cell);
        }
        if row_min > limit {
            return None;
        }
        prev2 = prev.clone();
        prev.copy_from_slice(&row);
    }
    (prev[m] <= limit).then_some(prev[m])
}

/// The nearest index term to a token the corpus does not carry, if one is
/// close enough to be trusted.
///
/// The scan is the whole term index with cheap gates in front of the edit
/// distance: a length window first, then the candidate's evidence (a term
/// fewer than five topics carry, or an idf under three, is not a word
/// someone was trying to spell — it is noise reaching back), then — for the
/// single-edit tier — an endpoint, which one edit cannot move both of.
/// What survives is ranked by fewest edits, then by the most postings
/// (a word people were searching for is a word the corpus talks about),
/// then by idf. The best one joins the query; two-finger typos are only
/// considered for the long clinical words, where a two-edit false positive
/// barely exists.
fn fuzzy_term(db: &Db, token: &str) -> Option<String> {
    let chars: Vec<char> = token.chars().collect();
    if chars.len() < 4 {
        return None;
    }
    let limit = if chars.len() >= 8 { 2 } else { 1 };
    let first = chars[0];
    let last = *chars.last().expect("non-empty by the length gate");
    let mut best: Option<(usize, std::cmp::Reverse<usize>, std::cmp::Reverse<f32>, String)> = None;
    for entry in &db.terms.entries {
        if entry.postings.len() < 5 || entry.idf < 3.0 {
            continue;
        }
        let cand: Vec<char> = entry.term.chars().collect();
        if cand.len().abs_diff(chars.len()) > limit {
            continue;
        }
        if limit == 1 && cand[0] != first && *cand.last().expect("non-empty") != last {
            continue;
        }
        let Some(edits) = damerau_within(&chars, &cand, limit) else {
            continue;
        };
        let rank = (
            edits,
            std::cmp::Reverse(entry.postings.len()),
            std::cmp::Reverse(entry.idf),
            entry.term.to_owned(),
        );
        if best.as_ref().is_none_or(|b| (rank.0, rank.1, rank.2) < (b.0, b.1, b.2)) {
            best = Some(rank);
        }
    }
    best.map(|(_, _, _, term)| term)
}

// ---------------------------------------------------------------------------
// The search itself
// ---------------------------------------------------------------------------

/// Bit flags on a hit, for the row badges: how this result was found.
pub mod found {
    /// The query text appears in the title or description.
    pub const LITERAL: u8 = 1 << 0;
    /// The TF-IDF cosine layer matched the meaning.
    pub const SEMANTIC: u8 = 1 << 1;
    /// The anatomy graph pulled it in as a neighbor of the leading organ.
    pub const GRAPH: u8 = 1 << 2;
    /// A synonym expansion or a prefix completion reached it.
    pub const EXPANDED: u8 = 1 << 3;
}

/// One ranked hit.
#[derive(Debug, Clone, Copy)]
pub struct Hit {
    /// Index into `Db::topics`.
    pub topic: usize,
    /// The score that placed it here.
    pub score: f32,
    /// Which layers found it — a union of [`found`] flags.
    pub flags: u8,
}

/// Where a plain-text needle lands in a haystack, for scoring.
fn find_plain(haystack_lower: &[char], needle_lower: &[char]) -> Option<(usize, bool)> {
    // Returns (char index, is_word_start).
    if needle_lower.is_empty() || haystack_lower.len() < needle_lower.len() {
        return None;
    }
    'outer: for start in 0..=(haystack_lower.len() - needle_lower.len()) {
        for (i, n) in needle_lower.iter().enumerate() {
            if haystack_lower[start + i] != *n {
                continue 'outer;
            }
        }
        let word_start = start == 0
            || !haystack_lower[start - 1].is_alphanumeric()
            || !haystack_lower[start].is_alphanumeric();
        return Some((start, word_start));
    }
    None
}

fn kind_boost(kind: char) -> f32 {
    match kind {
        'B' | 'S' => 0.65,
        'D' => 0.35,
        'X' | 'T' | 'P' => 0.18,
        'G' => 0.10,
        _ => 0.0,
    }
}

/// One query term with the weight it carries into the vector layer.
#[derive(Debug)]
struct QueryTerm {
    term: String,
    weight: f32,
    expanded: bool,
}

/// The query, understood: tokens, synonym expansions, typo corrections and
/// prefix completion.
///
/// Prefix completion is gated on nothing else having been expanded: a
/// partial word only ever ends a query while the person is still typing
/// it, and the moment a synonym or a correction has fired, the last token
/// is a *complete* word the dictionary already interpreted — completing
/// "peeing a lot" against the term index would be reaching for "lotion",
/// not finishing a thought.
fn understand(db: &Db, query: &str) -> Vec<QueryTerm> {
    let user = tokenize(query);
    let mut out: Vec<QueryTerm> = user
        .iter()
        .map(|t| QueryTerm { term: t.clone(), weight: 1.0, expanded: false })
        .collect();
    let mut expanded_any = false;
    // Tokens a matched synonym phrase already interpreted — the typo layer
    // must not second-guess the dictionary's reading of them ("cant sleep"
    // is understood; reinterpreting "cant" on its own is not deeper, it is
    // noisier).
    let mut interpreted: Vec<String> = Vec::new();

    let lowered = query.to_lowercase();
    for (phrase, expansion) in SYNONYMS {
        if lowered.contains(phrase) {
            expanded_any = true;
            interpreted.extend(tokenize(phrase));
            for t in tokenize(expansion) {
                if !out.iter().any(|q| q.term == t) {
                    out.push(QueryTerm { term: t, weight: 0.75, expanded: true });
                }
            }
        }
    }

    // The typo layer: every user token the index does not carry, and no
    // synonym has already spoken for, is offered its nearest index term.
    for t in &user {
        if interpreted.iter().any(|i| i == t) || db.terms.find(t).is_some() {
            continue;
        }
        if let Some(near) = fuzzy_term(db, t) {
            expanded_any = true;
            if !out.iter().any(|q| q.term == near) {
                out.push(QueryTerm { term: near, weight: 0.6, expanded: true });
            }
        }
    }

    // Prefix completion on the final token: "migra" -> "migraine" (and its
    // neighborhood). Only when the token itself is not a known term — a
    // known term is already scoring at full weight — and nothing above has
    // expanded the query.
    if !expanded_any {
        if let Some(last) = user.last() {
            if db.terms.find(last).is_none() && last.len() >= 3 {
                for term in db.terms.prefix(last, 6) {
                    if term != last {
                        out.push(QueryTerm {
                            term: term.to_owned(),
                            weight: 0.55,
                            expanded: true,
                        });
                    }
                }
            }
        }
    }
    out
}

/// The whole pipeline: understand the query, score every layer, walk the
/// graph from the leading hit, and return the best hits, best first.
#[must_use]
pub fn search(db: &Db, query: &str, limit: usize) -> (Vec<Hit>, bool) {
    let q = query.trim();
    if q.is_empty() {
        return (Vec::new(), false);
    }
    let is_regex = looks_like_regex(q);
    let regex = if is_regex { compile(q).ok() } else { None };
    // An unparseable regex is searched literally, and reported as such.
    let regex_failed = is_regex && regex.is_none();
    let effective_regex = regex;

    let needle: Vec<char> = q.to_lowercase().chars().collect();
    let terms = understand(db, q);

    // --- layer 1 + 2: literal and vector scores, in one pass ----------
    // `scores[i]` accumulates the cosine contribution of the query terms;
    // `flags[i]` remembers which layers spoke. A small fixed-capacity map
    // would be faster than a Vec of 11k f32, but 44 KB of bump allocation
    // per keystroke measures at under a millisecond even on the reference
    // phone — the simple thing wins.
    let mut semantic = vec![0.0_f32; db.topics.len()];
    let mut literal = vec![0.0_f32; db.topics.len()];
    let mut flags = vec![0_u8; db.topics.len()];

    for qt in &terms {
        if let Some(entry) = db.terms.find(&qt.term) {
            for &(topic, tf) in &entry.postings {
                let t = topic as usize;
                if t < db.topics.len() {
                    // A floor on the norm keeps two-word titles from
                    // out-cosining real answers: with tiny norms, one
                    // shared rare word is "all" of a short document.
                    let norm = db.terms.norms.get(t).copied().unwrap_or(1.0).max(2.5);
                    if norm > f32::EPSILON {
                        // q_i * d_i / |d| — the query side is constant
                        // across topics, so only the weight is needed.
                        // A posting at tf >= 3 carries the term in the
                        // *title* (the builder counts a title mention at
                        // weight 3, descriptions at 1): the page that IS
                        // the word outranks a page that merely says it
                        // often — a pill's leaflet says "depression"
                        // more than the Depression page does, and the
                        // cosine alone could not tell them apart.
                        let title = if tf >= 3.0 { 1.5 } else { 1.0 };
                        semantic[t] += qt.weight * entry.idf * (tf * entry.idf) * title / norm;
                        flags[t] |= if qt.expanded { found::EXPANDED } else { found::SEMANTIC };
                    }
                }
            }
        }
    }

    for (index, topic) in db.topics.iter().enumerate() {
        let mut matched = false;
        if let Some(re) = &effective_regex {
            if re.is_match(topic.title) {
                literal[index] += 2.6;
                matched = true;
            }
            if re.is_match(topic.description) {
                literal[index] += 0.6;
                matched = true;
            }
        } else {
            let title: Vec<char> = topic.title.to_lowercase().chars().collect();
            if let Some((at, word_start)) = find_plain(&title, &needle) {
                literal[index] += if at == 0 {
                    3.0
                } else if word_start {
                    2.2
                } else {
                    1.5
                };
                matched = true;
            }
            if !matched || needle.len() > 3 {
                let desc: Vec<char> = topic.description.to_lowercase().chars().collect();
                if let Some((at, word_start)) = find_plain(&desc, &needle) {
                    literal[index] += if at == 0 || word_start { 0.8 } else { 0.45 };
                    matched = true;
                }
            }
        }
        if matched {
            flags[index] |= found::LITERAL;
        }
    }

    // --- combine ---------------------------------------------------------
    // The literal layer is the precision instrument; the semantic layer is
    // the recall instrument. A topic neither layer scored is out.
    let mut hits: Vec<Hit> = Vec::new();
    for index in 0..db.topics.len() {
        let sem = semantic[index];
        let lit = literal[index];
        if sem <= f32::EPSILON && lit <= f32::EPSILON {
            continue;
        }
        let topic = &db.topics[index];
        // Cosine similarity of a short query runs 0.2..0.8 against a good
        // topic; scaled to sit beside the literal scores. The ceiling is a
        // guard against pathological density (a term repeated twenty times
        // in one description), not a blend control: an exact title match
        // collects both layers and cannot be outranked by this one, so the
        // cosine's own spread — which is the semantic layer's whole
        // contribution — is allowed to show. The old ceiling of 2.6 sat
        // below almost every strong cosine and tied them all, which handed
        // the ordering to the tiebreakers.
        let mut score = lit + (sem * 2.4).min(12.0);
        let len = topic.title.chars().count() as f32;
        score += kind_boost(topic.kind);
        score += (60.0 - len.min(60.0)) * 0.004;
        hits.push(Hit { topic: index, score, flags: flags[index] });
    }
    hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // --- layer 4: the graph walk -----------------------------------------
    // The strongest organ among the top hits names the region of interest;
    // its anatomical neighbors' topics ride along, weighted by edge. This
    // is what turns "chest pain" into a chest *area* answer rather than a
    // page list — a boost for neighbor-organ hits that are already in the
    // list, and a reserved cluster of seats at the end for the neighbors'
    // own pages, so a strong organ's flood of title hits cannot crowd out
    // the anatomy around it.
    let mut graph_cluster: Vec<Hit> = Vec::new();
    if hits.len() >= 6 {
        if let Some(leader) = hits.iter().take(3).find_map(|h| db.topics[h.topic].organ) {
            let neighbor: Vec<(usize, f32)> = db
                .graph
                .edges
                .get(leader)
                .cloned()
                .unwrap_or_default();
            if !neighbor.is_empty() {
                for hit in &mut hits {
                    if let Some(organ) = db.topics[hit.topic].organ {
                        if organ == leader {
                            // Already the center of attention.
                            continue;
                        }
                        if let Some((_, w)) = neighbor.iter().find(|(n, _)| *n == organ) {
                            hit.score += 0.5 * w;
                            hit.flags |= found::GRAPH;
                        }
                    }
                }
                hits.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
                // The neighbors' own pages: the best unscored topic of each
                // of the four strongest edges, flagged for the cluster.
                for &(organ, w) in neighbor.iter().take(4) {
                    let pick = db
                        .topics
                        .iter()
                        .enumerate()
                        .filter(|(i, t)| {
                            t.organ == Some(organ)
                                && semantic.get(*i).copied().unwrap_or(0.0) <= f32::EPSILON
                                && literal.get(*i).copied().unwrap_or(0.0) <= f32::EPSILON
                        })
                        .max_by(|(_ia, ta), (_ib, tb)| {
                            let la = 48.0 - ta.title.chars().count().min(48) as f32
                                + kind_boost(ta.kind);
                            let lb = 48.0 - tb.title.chars().count().min(48) as f32
                                + kind_boost(tb.kind);
                            la.partial_cmp(&lb).unwrap_or(std::cmp::Ordering::Equal)
                        });
                    if let Some((index, topic)) = pick {
                        let len = topic.title.chars().count().min(48) as f32;
                        graph_cluster.push(Hit {
                            topic: index,
                            score: 0.30 + 0.5 * w + kind_boost(topic.kind) + (48.0 - len) * 0.005,
                            flags: found::GRAPH,
                        });
                    }
                }
                graph_cluster.sort_by(|a, b| {
                    b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal)
                });
                graph_cluster.truncate(3);
            }
        }
    }

    let reserved = graph_cluster.len().min(limit / 4);
    hits.truncate(limit - reserved);
    hits.append(&mut graph_cluster);
    (hits, regex_failed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn m(pattern: &str, text: &str) -> bool {
        compile(pattern).expect("pattern compiles").is_match(text)
    }

    #[test]
    fn literals_and_case() {
        assert!(m("heart", "The HEART beats"));
        assert!(!m("heart", "lungs"));
    }

    #[test]
    fn classes_and_repeats() {
        assert!(m("colou?r", "color"));
        assert!(m("colou?r", "colour"));
        assert!(m("pain(s)?", "pain"));
        assert!(m("[a-z]+sion", "hypertension"));
        assert!(m("\\d{2,3}/\\d{2}", "blood pressure 120/80"));
        assert!(!m("^pain", "chest pain"));
        assert!(m("^chest", "chest pain"));
        assert!(m("pain$", "chest pain"));
    }

    #[test]
    fn alternation_and_groups() {
        assert!(m("(chest|lung) pain", "lung pain"));
        assert!(m("heart.*attack", "heart muscle attack"));
        assert!(m("\\bmass\\b", "a mass was found"));
        assert!(!m("\\bmass\\b", "massive"));
    }

    #[test]
    fn bad_patterns_are_reported_not_panicked() {
        assert!(compile("(unclosed").is_err());
        assert!(compile("[oops").is_err());
        assert!(compile("a{").is_ok(), "a lone {{ is a literal");
    }

    #[test]
    fn bounded_on_hostile_input() {
        // Classic catastrophic backtracking bait; the budget must cap it.
        let re = compile("(a+)+b").expect("compiles");
        let start = std::time::Instant::now();
        let hit = re.is_match(&"a".repeat(60));
        let _ = hit;
        assert!(start.elapsed().as_millis() < 500, "step budget caps the work");
    }

    #[test]
    fn detection() {
        assert!(looks_like_regex("heart.*attack"));
        assert!(looks_like_regex("^(chest|lung)"));
        assert!(!looks_like_regex("chest pain"));
        assert!(!looks_like_regex("..."));
    }

    // --- the semantic layer ---------------------------------------------

    /// Mirrored test vectors with build_db_semantic.py: the same text must
    /// tokenize identically on both sides, or the vectors and the queries
    /// stop agreeing.
    #[test]
    fn tokenizer_matches_the_builder() {
        // Builder: tokenize("Heart Attacks: Chest Pain") == [heart, attack, chest, pain]
        assert_eq!(
            tokenize("Heart Attacks: Chest Pain"),
            ["heart", "attack", "chest", "pain"]
        );
        // "studies" -> "study" (ies-rule with the y reattached)
        assert!(tokenize("Sleep Studies").contains(&"study".to_owned()));
        // "running" strips its "ing" on both sides ("runn" — the stem is
        // a key, not a word; what matters is that Rust and Python agree).
        assert_eq!(tokenize("running"), ["runn"]);
        assert_eq!(tokenize("medicated"), ["medicat"]);
        // stopwords drop, short words drop
        assert!(tokenize("the a of is it").is_empty());
        // punctuation splits
        assert_eq!(tokenize("kidney-stone (renal)"), ["kidney", "stone", "renal"]);
        // The plural fix: the singular and the plural must meet in one key.
        assert_eq!(tokenize("kidney stones"), ["kidney", "stone"]);
        assert_eq!(tokenize("headaches"), ["headache"]);
        assert_eq!(tokenize("muscles"), ["muscle"]);
        assert_eq!(tokenize("watches"), ["watch"]);
        assert_eq!(tokenize("boxes"), ["box"]);
        assert_eq!(tokenize("houses"), ["house"]);
    }

    #[test]
    fn semantic_finds_what_the_words_do_not_say() {
        let db = crate::db::parse();
        // No topic title contains the letters "tired all the time", but the
        // vector layer must surface fatigue topics.
        let (hits, _) = search(&db, "tired all the time", 8);
        assert!(!hits.is_empty(), "no hits for a semantic query");
        let in_top = hits.iter().take(6).any(|h| {
            let t = db.topics[h.topic].title.to_lowercase();
            t.contains("fatigue") || t.contains("tired") || t.contains("energy")
        });
        assert!(in_top, "fatigue not in top hits: {:?}", hits.iter().take(4).map(|h| db.topics[h.topic].title).collect::<Vec<_>>());
        // And the badge says how it was found.
        assert!(hits[0].flags & (found::SEMANTIC | found::LITERAL | found::EXPANDED) != 0);
    }

    #[test]
    fn synonyms_reach_the_clinical_spelling() {
        let db = crate::db::parse();
        // "heart attack" as typed matches "Myocardial Infarction" via the
        // synonym layer even though the literal letters differ.
        let (hits, _) = search(&db, "heart attack", 10);
        let top: Vec<&str> = hits.iter().take(5).map(|h| db.topics[h.topic].title.as_ref()).collect();
        assert!(
            top.iter().any(|t| t.to_lowercase().contains("heart attack") || t.to_lowercase().contains("myocardial")),
            "heart attack / myocardial infarction not surfaced: {top:?}"
        );
    }

    #[test]
    fn graph_pulls_in_the_neighbors() {
        let db = crate::db::parse();
        // The heart's neighbors in the corpus graph include blood and lungs;
        // a heart query must carry at least one graph-flagged hit.
        let (hits, _) = search(&db, "heart", 24);
        assert!(
            hits.iter().any(|h| h.flags & found::GRAPH != 0),
            "no graph-flagged hit for a heart query"
        );
    }

    #[test]
    fn prefix_completion_scores_partial_words() {
        let db = crate::db::parse();
        let (hits, _) = search(&db, "migra", 8);
        assert!(
            hits.iter().take(3).any(|h| db.topics[h.topic].title.to_lowercase().contains("migraine")),
            "migra did not surface migraine: {:?}",
            hits.iter().take(3).map(|h| db.topics[h.topic].title).collect::<Vec<_>>()
        );
    }

    #[test]
    fn literal_still_wins_exact_titles() {
        let db = crate::db::parse();
        let (hits, _) = search(&db, "chest pain", 8);
        let top = hits.first().map(|h| db.topics[h.topic].title.to_lowercase());
        assert!(top.as_deref().is_some_and(|t| t.contains("chest pain")), "exact title should lead: {top:?}");
    }

    // --- the typo layer --------------------------------------------------

    #[test]
    fn damerau_counts_the_classic_cases() {
        let d = |a: &str, b: &str, limit: usize| {
            damerau_within(
                &a.chars().collect::<Vec<_>>(),
                &b.chars().collect::<Vec<_>>(),
                limit,
            )
        };
        // Substitution, insertion, deletion: one edit each.
        assert_eq!(d("migrane", "migraine", 1), Some(1));
        assert_eq!(d("astma", "asthma", 1), Some(1));
        assert_eq!(d("kidny", "kidney", 1), Some(1));
        // A transposition is one edit, not two.
        assert_eq!(d("teh", "the", 1), Some(1));
        // Beyond the limit is a refusal, not a distance.
        assert_eq!(d("migrane", "migraine", 0), None);
        assert_eq!(d("heart", "lungs", 1), None);
    }

    #[test]
    fn typos_still_find_the_word() {
        let db = crate::db::parse();
        // The clinical vocabulary is the part people cannot spell; each of
        // these is a real-world misspelling with zero literal overlap.
        for (query, want) in [
            ("migrane", "migraine"),
            ("dyabetes", "diabetes"),
            ("pnemonia", "pneumonia"),
            ("hipatitis", "hepatitis"),
            ("aneamia", "anemia"),
        ] {
            let (hits, _) = search(&db, query, 8);
            let top: Vec<&str> = hits.iter().take(3).map(|h| db.topics[h.topic].title.as_ref()).collect();
            assert!(
                top.iter().any(|t| t.to_lowercase().contains(want)),
                "{query} did not surface {want}: {top:?}"
            );
        }
        // Two tokens, one misspelled: the good token still carries, the bad
        // one is corrected, and the exact page leads.
        let (hits, _) = search(&db, "kidny stones", 8);
        let top = hits.first().map(|h| db.topics[h.topic].title.to_lowercase());
        assert!(
            top.as_deref().is_some_and(|t| t.contains("kidney stones")),
            "kidny stones should find Kidney Stones first: {top:?}"
        );
    }

    #[test]
    fn typos_do_not_reach_for_gibberish() {
        let db = crate::db::parse();
        // Nothing in the corpus is one edit from this; the layer must stay
        // quiet rather than invent a reading.
        let (hits, _) = search(&db, "xqkjw", 8);
        assert!(hits.is_empty(), "gibberish must not reach: {:?}",
            hits.iter().take(3).map(|h| db.topics[h.topic].title).collect::<Vec<_>>());
    }

    #[test]
    fn razor_burn_fires_at_query_start() {
        let db = crate::db::parse();
        // The phrase used to carry a leading space and never matched at the
        // start of a query — the one place it is most likely to sit.
        let (hits, _) = search(&db, "razor burn", 8);
        let top: Vec<&str> = hits.iter().take(3).map(|h| db.topics[h.topic].title.as_ref()).collect();
        assert!(
            top.iter().any(|t| t.to_lowercase().contains("razor burn")),
            "razor burn not surfaced: {top:?}"
        );
        assert!(
            hits.iter().any(|h| h.flags & found::EXPANDED != 0),
            "the synonym layer never fired for a query-start 'razor burn'"
        );
    }

    // --- title evidence in the vector layer -------------------------------

    #[test]
    fn the_page_that_is_the_word_leads() {
        let db = crate::db::parse();
        // A dense medication leaflet used to out-cosine the page named
        // after the condition, because the cosine was capped into a tie.
        for (query, want) in [
            ("feeling sad", "depression"),
            ("peeing a lot", "frequent urination"),
            ("cant sleep", "insomnia"),
        ] {
            let (hits, _) = search(&db, query, 8);
            let top: Vec<&str> = hits.iter().take(3).map(|h| db.topics[h.topic].title.as_ref()).collect();
            assert!(
                top.iter().any(|t| t.to_lowercase().contains(want)),
                "{query} did not surface {want} in the top three: {top:?}"
            );
        }
    }
}
