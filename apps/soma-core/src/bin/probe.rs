// One-off probes while tuning the search stack. Not part of the app.

use soma_core::db;
use soma_core::search;
use std::time::Instant;

fn main() {
    let database = db::parse();

    // --- latency: where the milliseconds are ------------------------------
    // Each query runs twice: the plain word (no typo layer) and its
    // misspelling (the typo layer scans the term index). The delta is what
    // the fuzzy layer costs; the shared floor is the literal scan.
    let pairs = [
        ("migraine", "migrane"),
        ("diabetes", "dyabetes"),
        ("kidney stones", "kidny stones"),
        ("chest pain", "chest pian"),
    ];
    println!("== latency (release timings, 50 runs each)");
    for (plain, typo) in pairs {
        let t0 = Instant::now();
        for _ in 0..50 {
            let _ = search::search(&database, plain, 24);
        }
        let plain_us = t0.elapsed().as_micros() / 50;
        let t1 = Instant::now();
        for _ in 0..50 {
            let _ = search::search(&database, typo, 24);
        }
        let typo_us = t1.elapsed().as_micros() / 50;
        println!(
            "   {plain:<15} {plain_us:>5} µs   {typo:<15} {typo_us:>5} µs   (fuzzy +{} µs)",
            typo_us - plain_us
        );
    }

    // --- the demo queries, as the app shows them ---------------------------
    for q in [
        "chest pain",
        "tired all the time",
        "heart attack",
        "stomach ache",
        "migra",
        "migrane",
        "kidney stone",
        "short of breath",
        "cant sleep",
    ] {
        let (hits, fell_back) = search::search(&database, q, 8);
        println!(
            "== {q}{}",
            if fell_back {
                "  (regex fell back to literal)"
            } else {
                ""
            }
        );
        for h in hits.iter().take(6) {
            let t = &database.topics[h.topic];
            let organ = t.organ.map(|o| database.organs[o].name).unwrap_or("-");
            let tags = [
                (h.flags & search::found::LITERAL != 0, "text"),
                (h.flags & search::found::SEMANTIC != 0, "meaning"),
                (h.flags & search::found::GRAPH != 0, "graph"),
                (h.flags & search::found::EXPANDED != 0, "expanded"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| *n)
            .collect::<Vec<_>>()
            .join("+");
            println!(
                "   {score:6.2} [{tags:>18}] {organ:<18} {title}",
                score = h.score,
                tags = tags,
                organ = organ,
                title = t.title
            );
        }
        println!();
    }
}
