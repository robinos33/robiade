//! Mesure le temps de lecture de l'historique réel : `cargo run --release --example scan_history`.

use robiade_adapters::claude_code::TranscriptStore;
use robiade_core::history::TranscriptReader;

fn main() {
    let root = std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".claude/projects");
    let store = TranscriptStore::new(root);
    for pass in ["à froid", "avec cache"] {
        let start = std::time::Instant::now();
        let all = store.summaries().unwrap();
        println!("{pass} : {} sessions en {:?}", all.len(), start.elapsed());
    }
    for s in store.summaries().unwrap().iter().take(3) {
        println!("- {} | {} | {:?} | {} tokens", s.title, s.project_path, s.git_branch, s.tokens.total());
    }
}
