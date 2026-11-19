// Debug: run several queries and dump the ranked answers with their flags.
fn main() {
    let db = soma::db::parse();
    for q in ["chest pain", "tired all the time", "heart attack", "stomach ache", "migra", "kidney stone", "short of breath", "cant sleep"] {
        let (hits, _) = soma::search::search(&db, q, 8);
        println!("== {q}");
        for h in hits.iter().take(6) {
            let t = &db.topics[h.topic];
            let organ = t.organ.map(|o| db.organs[o].name).unwrap_or("-");
            let tags = [
                (h.flags & 1 != 0, "text"),
                (h.flags & 2 != 0, "meaning"),
                (h.flags & 4 != 0, "graph"),
                (h.flags & 8 != 0, "expanded"),
            ]
            .iter()
            .filter(|(on, _)| *on)
            .map(|(_, n)| *n)
            .collect::<Vec<_>>()
            .join("+");
            println!("   {:6.2} [{tags: >18}] {organ: <18} {}", h.score, t.title);
        }
        println!();
    }
}
