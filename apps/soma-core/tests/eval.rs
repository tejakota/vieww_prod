//! The search-quality benchmark: a fixed query set with expected answers,
//! scored top-1 / top-3 / top-8 by group, with a floor that fails the build
//! when ranking regresses.
//!
//! Every case is a question a person would actually type and the corpus page
//! that should answer it (verified against `soma/assets/db/soma_db.txt`).
//! A case passes at k when any of the first k hits' titles contains the
//! expected substring, case-insensitively — the same judgement a person
//! makes skimming the list, formalized.
//!
//! Run the report: `cargo test --test eval -- --nocapture`
//! The same command without `--nocapture` is the gate.

use soma_core::db;
use soma_core::search;

#[derive(Clone, Copy, PartialEq)]
enum Group {
    /// The query is the title, or near it.
    Exact,
    /// Everyday words for a clinical page ("pink eye" → Conjunctivitis).
    Synonym,
    /// Nothing in the query appears in the answer's title — the vector
    /// layer has to do the work ("tired all the time" → Fatigue).
    Semantic,
    /// The last word is still being typed.
    Prefix,
    /// The clinical word is misspelled.
    Typo,
}

impl Group {
    fn name(self) -> &'static str {
        match self {
            Group::Exact => "exact",
            Group::Synonym => "synonym",
            Group::Semantic => "semantic",
            Group::Prefix => "prefix",
            Group::Typo => "typo",
        }
    }
}

struct Case {
    query: &'static str,
    /// Substring that must appear in a top-k hit's title to pass.
    expect: &'static str,
    group: Group,
}

const CASES: &[Case] = &[
    // --- exact: the query is (almost) the title -------------------------
    Case {
        query: "chest pain",
        expect: "Chest Pain",
        group: Group::Exact,
    },
    Case {
        query: "kidney stones",
        expect: "Kidney Stones",
        group: Group::Exact,
    },
    Case {
        query: "diabetes",
        expect: "Diabetes",
        group: Group::Exact,
    },
    Case {
        query: "asthma",
        expect: "Asthma",
        group: Group::Exact,
    },
    Case {
        query: "appendicitis",
        expect: "Appendicitis",
        group: Group::Exact,
    },
    Case {
        query: "psoriasis",
        expect: "Psoriasis",
        group: Group::Exact,
    },
    // --- synonym: everyday words, clinical page -------------------------
    Case {
        query: "heart attack",
        expect: "Heart Attack",
        group: Group::Synonym,
    },
    Case {
        query: "pink eye",
        expect: "Conjunctivitis",
        group: Group::Synonym,
    },
    Case {
        query: "cant sleep",
        expect: "Insomnia",
        group: Group::Synonym,
    },
    Case {
        query: "throwing up",
        expect: "Nausea",
        group: Group::Synonym,
    },
    Case {
        query: "sore throat",
        expect: "Sore Throat",
        group: Group::Synonym,
    },
    Case {
        query: "high blood pressure",
        expect: "Hypertension",
        group: Group::Synonym,
    },
    Case {
        query: "kidney stone",
        expect: "Kidney Stones",
        group: Group::Synonym,
    },
    Case {
        query: "snoring",
        expect: "Snoring",
        group: Group::Synonym,
    },
    Case {
        query: "razor burn",
        expect: "Razor Burn",
        group: Group::Synonym,
    },
    // --- semantic: the answer's title shares no query word --------------
    Case {
        query: "tired all the time",
        expect: "Fatigue",
        group: Group::Semantic,
    },
    Case {
        query: "short of breath",
        expect: "Dyspnea",
        group: Group::Semantic,
    },
    Case {
        query: "peeing a lot",
        expect: "Frequent Urination",
        group: Group::Semantic,
    },
    Case {
        query: "spots on skin",
        expect: "Acne",
        group: Group::Semantic,
    },
    Case {
        query: "feeling sad",
        expect: "Depression",
        group: Group::Semantic,
    },
    Case {
        query: "forgetting things",
        expect: "Memory Loss",
        group: Group::Semantic,
    },
    // --- prefix: still typing -------------------------------------------
    Case {
        query: "migra",
        expect: "Migraine",
        group: Group::Prefix,
    },
    Case {
        query: "insomn",
        expect: "Insomnia",
        group: Group::Prefix,
    },
    // --- typo: the clinical word is misspelled --------------------------
    Case {
        query: "migrane",
        expect: "Migraine",
        group: Group::Typo,
    },
    Case {
        query: "dyabetes",
        expect: "Diabetes",
        group: Group::Typo,
    },
    Case {
        query: "kidny stones",
        expect: "Kidney Stones",
        group: Group::Typo,
    },
    Case {
        query: "pnemonia",
        expect: "Pneumonia",
        group: Group::Typo,
    },
    Case {
        query: "broncitis",
        expect: "Bronchitis",
        group: Group::Typo,
    },
    Case {
        query: "astma attack",
        expect: "Asthma",
        group: Group::Typo,
    },
    Case {
        query: "hipatitis",
        expect: "Hepatitis",
        group: Group::Typo,
    },
    Case {
        query: "aneamia",
        expect: "Anemia",
        group: Group::Typo,
    },
];

#[test]
fn search_quality_eval() {
    let database = db::parse();
    let mut failures: Vec<String> = Vec::new();
    let mut total_us = 0_u128;

    println!();
    println!(
        "{:<22} {:<7} {:>4} {:>4} {:>4}   {}",
        "query", "group", "t1", "t3", "t8", "first miss"
    );
    println!("{}", "-".repeat(78));

    for case in CASES {
        let start = std::time::Instant::now();
        let (hits, _) = search::search(&database, case.query, 24);
        total_us += start.elapsed().as_micros();

        let title_at = |i: usize| -> Option<&str> {
            hits.get(i).map(|h| database.topics[h.topic].title.as_ref())
        };
        let hit_at = |k: usize| -> bool {
            (0..k).any(|i| {
                title_at(i).is_some_and(|t| t.to_lowercase().contains(&case.expect.to_lowercase()))
            })
        };
        let top1 = hit_at(1);
        let top3 = hit_at(3);
        let top8 = hit_at(8);

        let first_miss = if top1 {
            String::new()
        } else {
            format!(
                "#1 was {:?} (want ~{:?})",
                title_at(0).unwrap_or("<nothing>"),
                case.expect
            )
        };
        println!(
            "{:<22} {:<7} {:>4} {:>4} {:>4}   {}",
            case.query,
            case.group.name(),
            yn(top1),
            yn(top3),
            yn(top8),
            first_miss
        );

        if !top3 {
            failures.push(format!(
                "[{}] {:?} — no {:?} in top 3 (top: {:?})",
                case.group.name(),
                case.query,
                case.expect,
                title_at(0).unwrap_or("<nothing>")
            ));
        }
    }

    // --- the summary table ------------------------------------------------
    let group_names = ["exact", "synonym", "semantic", "prefix", "typo"];
    println!("{}", "-".repeat(78));
    let mut overall = [0_usize; 4];
    for g in group_names {
        let cases = CASES.iter().filter(|c| c.group.name() == g).count();
        let (mut t1, mut t3, mut t8) = (0, 0, 0);
        for case in CASES.iter().filter(|c| c.group.name() == g) {
            let (hits, _) = search::search(&database, case.query, 24);
            let contains = |k: usize| {
                (0..k).any(|i| {
                    hits.get(i).is_some_and(|h| {
                        database.topics[h.topic]
                            .title
                            .to_lowercase()
                            .contains(&case.expect.to_lowercase())
                    })
                })
            };
            t1 += usize::from(contains(1));
            t3 += usize::from(contains(3));
            t8 += usize::from(contains(8));
        }
        println!(
            "{:<9} {:>2}/{:<2} top1  {:>2}/{:<2} top3  {:>2}/{:<2} top8",
            g, t1, cases, t3, cases, t8, cases
        );
        overall[0] += t1;
        overall[1] += t3;
        overall[2] += t8;
        overall[3] += cases;
    }
    let avg_us = total_us / CASES.len() as u128;
    println!(
        "overall   {:>2}/{:<2} top1  {:>2}/{:<2} top3  {:>2}/{:<2} top8   ({} µs/query avg)",
        overall[0], overall[3], overall[1], overall[3], overall[2], overall[3], avg_us
    );
    println!();

    // --- the floor: what a regression costs ------------------------------
    // Measured floors, not aspirations — each is the current engine's
    // number. Raise them when the engine improves; a drop fails here.
    // (v0.2 baseline, before the typo layer, the title-evidence fix and
    // the plural stemmer fix, was 18/31 top-3 with the typo group at 1/8.)
    let n = CASES.len();
    assert!(
        overall[1] >= 29,
        "top-3 recall fell below the floor: {}/{} — failures:\n{}",
        overall[1],
        n,
        failures.join("\n")
    );
    assert!(
        overall[0] >= 26,
        "top-1 recall fell below the floor: {}/{} — failures:\n{}",
        overall[0],
        n,
        failures.join("\n")
    );
    for (group, floor) in [
        (Group::Exact, 6),
        (Group::Synonym, 9),
        (Group::Semantic, 4),
        (Group::Prefix, 2),
        (Group::Typo, 8),
    ] {
        let t3 = group_pass(group, 3, &database);
        let total = CASES.iter().filter(|c| c.group == group).count();
        assert!(
            t3 >= floor,
            "[{}] top-3 fell below the floor: {}/{}",
            group.name(),
            t3,
            total
        );
    }
}

fn group_pass(group: Group, k: usize, database: &db::Db) -> usize {
    CASES
        .iter()
        .filter(|c| c.group == group)
        .map(|case| {
            let (hits, _) = search::search(database, case.query, 24);
            usize::from((0..k).any(|i| {
                hits.get(i).is_some_and(|h| {
                    database.topics[h.topic]
                        .title
                        .to_lowercase()
                        .contains(&case.expect.to_lowercase())
                })
            }))
        })
        .sum()
}

fn yn(v: bool) -> &'static str {
    if v {
        "y"
    } else {
        "-"
    }
}
