//! `#[vieww_audio::realtime]` (AU-18): the real-time contract of a render
//! function, checked by the compiler and, in debug builds, at run time.
//!
//! **Compile time.** The function body may not name what allocates or
//! blocks on an audio thread: `vec!`/`format!`/`println!`-family macros,
//! `Box::new`, `Vec::new`/`with_capacity`, `String::new`/`from`,
//! `Arc::new`, `Rc::new`, collection constructors, and the methods
//! `collect`, `to_vec`, `to_owned`, `to_string`, `push`, `extend`,
//! `insert`, `resize`, `reserve`, `lock`, `recv`, `join`, `sleep`, `wait`.
//! A match is a compile error at the call, saying which rule. It is a
//! token check — it sees what the function itself writes, not what the
//! functions it calls do (those carry the attribute themselves, or are
//! covered by `tests/realtime_no_alloc.rs`). A deliberate exception is
//! named in the attribute: `#[realtime(allow(collect))]`.
//!
//! **Run time (debug builds).** The body runs inside
//! `vieww_audio::rt::RtSection`, and an app or test that installs
//! `vieww_audio::rt::CheckedAlloc` as its global allocator gets a panic,
//! naming the function, when anything allocates inside the section — the
//! callees included. Release builds compile the section away.

use proc_macro::{Delimiter, Group, Ident, Literal, Punct, Spacing, Span, TokenStream, TokenTree};

const BANNED_MACROS: &[&str] = &[
    "vec", "format", "println", "eprintln", "print", "eprint", "dbg", "panic",
];
const BANNED_TYPES: &[&str] = &[
    "Box", "Vec", "String", "Arc", "Rc", "HashMap", "HashSet", "BTreeMap", "BTreeSet", "VecDeque",
];
const BANNED_CTORS: &[&str] = &["new", "with_capacity", "from", "from_iter", "default"];
const BANNED_METHODS: &[&str] = &[
    "collect",
    "to_vec",
    "to_owned",
    "to_string",
    "push",
    "extend",
    "insert",
    "resize",
    "reserve",
    "lock",
    "recv",
    "join",
    "sleep",
    "wait",
];

fn error(span: Span, msg: &str) -> TokenStream {
    // compile_error!("msg") at `span`.
    let mut ident = TokenTree::Ident(Ident::new("compile_error", span));
    ident.set_span(span);
    let mut bang = TokenTree::Punct(Punct::new('!', Spacing::Alone));
    bang.set_span(span);
    let mut lit = TokenTree::Literal(Literal::string(msg));
    lit.set_span(span);
    let mut args = TokenTree::Group(Group::new(Delimiter::Parenthesis, lit.into()));
    args.set_span(span);
    let mut semi = TokenTree::Punct(Punct::new(';', Spacing::Alone));
    semi.set_span(span);
    [ident, bang, args, semi].into_iter().collect()
}

/// The first rule `body` breaks, with where.
fn scan(body: TokenStream, allow: &[String], errors: &mut Vec<(Span, String)>) {
    let tokens: Vec<TokenTree> = body.into_iter().collect();
    for (i, t) in tokens.iter().enumerate() {
        match t {
            TokenTree::Group(g) => scan(g.stream(), allow, errors),
            TokenTree::Ident(id) => {
                let name = id.to_string();
                let next = tokens.get(i + 1);
                let prev = i.checked_sub(1).and_then(|k| tokens.get(k));
                let is_bang = matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == '!');
                if is_bang && BANNED_MACROS.contains(&name.as_str()) && !allow.contains(&name) {
                    errors.push((
                        id.span(),
                        format!(
                            "`{name}!` allocates or blocks; not allowed in a #[realtime] function"
                        ),
                    ));
                }
                // `Type::ctor`
                let path = matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == ':')
                    && matches!(tokens.get(i + 2), Some(TokenTree::Punct(p)) if p.as_char() == ':');
                if path && BANNED_TYPES.contains(&name.as_str()) {
                    if let Some(TokenTree::Ident(ctor)) = tokens.get(i + 3) {
                        let c = ctor.to_string();
                        let key = format!("{name}::{c}");
                        if BANNED_CTORS.contains(&c.as_str())
                            && !allow.contains(&key)
                            && !allow.contains(&name)
                        {
                            errors.push((
                                id.span(),
                                format!("`{key}` allocates; not allowed in a #[realtime] function"),
                            ));
                        }
                    }
                }
                // `.method(` and `thread::sleep(`
                let dotted = matches!(prev, Some(TokenTree::Punct(p)) if p.as_char() == '.');
                let pathed = matches!(prev, Some(TokenTree::Punct(p)) if p.as_char() == ':');
                let called = matches!(next, Some(TokenTree::Group(g)) if g.delimiter() == Delimiter::Parenthesis)
                    || matches!(next, Some(TokenTree::Punct(p)) if p.as_char() == ':');
                if (dotted || (pathed && name == "sleep"))
                    && called
                    && BANNED_METHODS.contains(&name.as_str())
                    && !allow.contains(&name)
                {
                    errors.push((id.span(), format!("`{name}` may allocate or block; not allowed in a #[realtime] function (or name it: #[realtime(allow({name}))])")));
                }
            }
            _ => {}
        }
    }
}

/// `allow(a, b)` from the attribute's arguments.
fn allowed(attr: TokenStream) -> Vec<String> {
    let mut out = Vec::new();
    let tokens: Vec<TokenTree> = attr.into_iter().collect();
    for (i, t) in tokens.iter().enumerate() {
        if let (TokenTree::Ident(id), Some(TokenTree::Group(g))) = (t, tokens.get(i + 1)) {
            if id.to_string() == "allow" {
                let mut buf = String::new();
                for x in g.stream() {
                    match x {
                        TokenTree::Punct(p) if p.as_char() == ',' => {
                            out.push(std::mem::take(&mut buf));
                        }
                        other => buf.push_str(&other.to_string()),
                    }
                }
                if !buf.is_empty() {
                    out.push(buf);
                }
            }
        }
    }
    out.into_iter().map(|s| s.replace(' ', "")).collect()
}

/// Mark a function as real-time. See the crate documentation.
#[proc_macro_attribute]
pub fn realtime(attr: TokenStream, item: TokenStream) -> TokenStream {
    let allow = allowed(attr);
    let mut tokens: Vec<TokenTree> = item.into_iter().collect();
    // The function's name (the identifier after `fn`) and body (its last
    // brace group).
    let name = tokens
        .windows(2)
        .find_map(|w| match (&w[0], &w[1]) {
            (TokenTree::Ident(f), TokenTree::Ident(n)) if f.to_string() == "fn" => {
                Some(n.to_string())
            }
            _ => None,
        })
        .unwrap_or_else(|| "?".into());
    let Some(body_at) = tokens
        .iter()
        .rposition(|t| matches!(t, TokenTree::Group(g) if g.delimiter() == Delimiter::Brace))
    else {
        return error(
            Span::call_site(),
            "#[realtime] goes on a function with a body",
        );
    };
    let TokenTree::Group(body) = tokens[body_at].clone() else {
        unreachable!("found as a group");
    };
    let mut errors = Vec::new();
    scan(body.stream(), &allow, &mut errors);
    if !errors.is_empty() {
        return errors.into_iter().flat_map(|(s, m)| error(s, &m)).collect();
    }
    // `let __vieww_rt = ::vieww_audio::rt::RtSection::enter("name");` then
    // the body as it was.
    let guard: TokenStream =
        format!("let __vieww_rt_section = ::vieww_audio::rt::RtSection::enter({name:?});")
            .parse()
            .expect("a statement");
    let mut stream = guard;
    stream.extend(body.stream());
    let mut new_body = Group::new(Delimiter::Brace, stream);
    new_body.set_span(body.span());
    tokens[body_at] = TokenTree::Group(new_body);
    tokens.into_iter().collect()
}
