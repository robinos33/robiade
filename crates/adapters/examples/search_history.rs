//! Mesure l'index de recherche sur les vrais transcripts : `cargo run --release --example search_history -- <db> <mots…>`.

use std::time::Instant;

use robiade_adapters::claude_code::TranscriptStore;
use robiade_adapters::sqlite_search::SearchIndex;
use robiade_core::history::SessionSearch;

fn main() {
    let mut args = std::env::args().skip(1);
    let db = args.next().expect("chemin de la base");
    let query = args.collect::<Vec<_>>().join(" ");
    let home = std::env::var("HOME").expect("HOME");
    let store = TranscriptStore::new(format!("{home}/.claude/projects").into());
    let index = SearchIndex::open(db.as_ref()).expect("ouverture de l'index");

    let t = Instant::now();
    let files = store.transcript_files();
    let changed = index.sync(&files).expect("synchronisation");
    println!("sync : {changed}/{} transcripts relus en {:?}", files.len(), t.elapsed());

    let t = Instant::now();
    let hits = index.search(&query, 50).expect("recherche");
    println!("recherche « {query} » : {} sessions en {:?}", hits.len(), t.elapsed());
    for h in hits.iter().take(3) {
        println!("  {} {}", h.session_id, h.snippet.replace(['\u{2}', '\u{3}'], "*"));
    }
}
