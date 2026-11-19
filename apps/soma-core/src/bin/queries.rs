// Run the sample queries and dump the ranked answers with their found-by
// flags — the same view of the engine `soma/examples/graph_debug.rs` prints,
// on the same files, without linking the app (which needs the vieww
// workspace). Any ranking change shows up here first.

use soma_core::db;
use soma_core::search;

fn main() {
    let database = db::parse();
    let queries = [
        "chest pain",
        "tired all the time",
        "heart attack",
        "stomach ache",
        "migra",
        "kidney stone",
        "short of breath",
        "cant sleep",
    ];
    for q in queries {
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
                "   {score:6.2} [{tags: >18}] {organ: <18} {title}",
                score = h.score,
                tags = tags,
                organ = organ,
                title = t.title
            );
        }
        println!();
    }
}
