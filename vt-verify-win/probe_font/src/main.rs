// Mirror of render::load_font_styled's query chain, for recording the actual
// family fontdb resolves on this Windows host (2A handover item: windows.toml
// fonts missing -> fontdb actual family name record) + CJK glyph coverage.
use fontdb::{Database, Family, Query, Source};
use fontdue::FontSettings;

fn show(db: &Database, label: &str, q: &Query) {
    match db.query(q) {
        Some(id) => {
            let f = db.face(id).unwrap();
            let bytes = match &f.source {
                Source::File(p) => std::fs::read(p).unwrap(),
                Source::Binary(b) => b.as_ref().as_ref().to_vec(),
                Source::SharedFile(_, d) => d.as_ref().as_ref().to_vec(),
            };
            let font = fontdue::Font::from_bytes(bytes.as_slice(), FontSettings::default()).unwrap();
            println!(
                "{} -> family='{}' ps='{}' mono={} style={:?} has_han={}",
                label, f.families[0].0, f.post_script_name, f.monospaced, f.style,
                font.has_glyph('\u{6C49}')
            );
        }
        None => println!("{} -> None", label),
    }
}

fn main() {
    let mut db = Database::new();
    db.load_system_fonts();
    for fam in ["Microsoft YaHei Mono", "Microsoft YaHei", "Consolas"] {
        let q = Query {
            families: &[Family::Name(fam), Family::Monospace],
            ..Default::default()
        };
        show(&db, &format!("name('{}')+Monospace", fam), &q);
    }
    for g in [Family::Monospace, Family::SansSerif] {
        let q = Query { families: &[g], ..Default::default() };
        show(&db, &format!("generic({:?})", g), &q);
    }
}
