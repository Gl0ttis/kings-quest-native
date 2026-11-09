//! vocabdump DISK: every verb and noun id with its words (the PCjr vocabulary).

fn main() {
    let a: Vec<String> = std::env::args().skip(1).collect();
    let disk = kq1_pcjr::disk::Disk::from_file(std::fs::read(&a[0]).expect("disk")).expect("disk");
    let words = kq1_pcjr::vocab::read(&disk.code_segment()).expect("vocab");
    let n = kq1_formats::vocab::Names::new(&words);
    for (kind, list) in [("verb", &n.verbs), ("noun", &n.nouns)] {
        for (id, w) in list.iter().enumerate().filter(|(_, w)| !w.is_empty()) {
            println!("{kind} {id}: {}", w.join(" | "));
        }
    }
}
