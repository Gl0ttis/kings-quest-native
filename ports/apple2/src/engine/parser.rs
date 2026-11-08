//! Command-line parser ($514B), working on the raw vocabulary bytes (fixed resource at
//! $0800) exactly as the engine does: alphabetical search with early exit, phrase entries,
//! later words overwriting earlier ones.

/// Result of $514B: verb ($9A), noun ($9B), and the unknown word if parsing failed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Parsed {
    pub verb: u8,
    pub noun: u8,
    pub unknown: Option<String>,
    /// The low byte the word compare ($5232) last stored into its own `CMP $6A70,X`
    /// operand at $523F: $70 for a first word ($5228), $84 for a phrase's second word
    /// ($522E). None if no compare ran.
    pub cmp_table: Option<u8>,
}

impl Parsed {
    /// The routine's return value: true when nothing usable was found (or a word was
    /// unknown), so the parser rules are not run.
    pub fn nothing(&self) -> bool {
        self.unknown.is_some() || (self.verb == 0 && self.noun == 0)
    }
}

struct Entry {
    pos: u8,
    kind: u8,
    id: u8,
}

enum Cmp {
    Match(Entry),
    Less,     // entry sorts before the word: skipped, keep searching
    NotFound, // entry sorts after the word
}

/// $5232: compare the entry at `ptr` with `word`; on a match or a skip, advance `ptr`.
fn compare(v: &[u8], ptr: &mut usize, word: &[u8]) -> Cmp {
    let at = |i: usize| v.get(i).copied().unwrap_or(0);
    let mut y = 0;
    loop {
        let e = at(*ptr + y);
        let w = word.get(y).copied().unwrap_or(0);
        if e == w {
            if e == 0 {
                let entry = Entry { pos: at(*ptr + y + 1), kind: at(*ptr + y + 2), id: at(*ptr + y + 3) };
                *ptr += y + 4;
                return Cmp::Match(entry);
            }
            y += 1;
            continue;
        }
        if e > w {
            return Cmp::NotFound;
        }
        // $524D: skip this entry; a phrase start (position 2) also skips the next entry.
        loop {
            let mut y = 0;
            while at(*ptr + y) != 0 {
                y += 1;
            }
            let pos = at(*ptr + y + 1);
            *ptr += y + 4;
            if pos != 2 || *ptr >= v.len() {
                break;
            }
        }
        return Cmp::Less;
    }
}

/// $521D: search forward from `ptr` for `word`. The engine would walk past the end of the
/// list into the memory that follows; here the end of the list means "not found".
fn lookup(v: &[u8], ptr: &mut usize, word: &[u8], table: &mut Option<u8>) -> Option<Entry> {
    *table = Some(0x70); // $5228
    loop {
        if *ptr >= v.len() || v[*ptr] == 0 {
            return None;
        }
        match compare(v, ptr, word) {
            Cmp::Match(e) => return Some(e),
            Cmp::Less => continue,
            Cmp::NotFound => return None,
        }
    }
}

/// $51E2/$51E8: the next word from `pos`, lowercased (ORA #$20), at most 19 characters.
/// A longer word is split: the rest is read as the next word, as in the engine.
fn next_word(line: &[u8], pos: &mut usize) -> Option<Vec<u8>> {
    let at = |i: usize| line.get(i).copied().unwrap_or(0);
    let mut x = *pos;
    loop {
        match at(x) {
            0 => return None,
            b' ' => x += 1,
            _ => break,
        }
    }
    let mut w = Vec::new();
    while at(x) != 0 && at(x) != b' ' && w.len() < 19 {
        w.push(at(x) | 0x20);
        x += 1;
    }
    *pos = x;
    Some(w)
}

/// $514B
pub fn parse(vocab: &[u8], line: &[u8]) -> Parsed {
    let (mut verb, mut noun) = (0u8, 0u8);
    let mut pos = 0;
    let store = |e: &Entry, verb: &mut u8, noun: &mut u8| match e.kind {
        4 => {}
        3 => *noun = e.id,
        _ => *verb = e.id,
    };
    let mut table = None;
    let unknown = |w: &[u8], t: Option<u8>| Parsed {
        verb: 0xFF,
        noun: 0,
        unknown: Some(w.iter().map(|&c| c as char).collect()),
        cmp_table: t,
    };
    loop {
        if noun != 0 && verb != 0 {
            break;
        }
        let Some(w1) = next_word(line, &mut pos) else { break };
        let mut ptr = 0;
        let mut stored = 0;
        let Some(mut e) = lookup(vocab, &mut ptr, &w1, &mut table) else {
            return Parsed { noun, ..unknown(&w1, table) };
        };
        if e.pos == 1 {
            stored = 1;
            store(&e, &mut verb, &mut noun);
            match lookup(vocab, &mut ptr, &w1, &mut table) {
                Some(e2) => e = e2,
                None => continue,
            }
        }
        let _ = e;
        // $5181: try to complete a phrase with the following word.
        let after_w1 = pos;
        if let Some(w2) = next_word(line, &mut pos) {
            loop {
                table = Some(0x84); // $522E
                match compare(vocab, &mut ptr, &w2) {
                    Cmp::Match(e3) => {
                        stored = 2;
                        store(&e3, &mut verb, &mut noun);
                        break;
                    }
                    Cmp::NotFound => break,
                    Cmp::Less => {
                        if lookup(vocab, &mut ptr, &w1, &mut table).is_none() {
                            break;
                        }
                    }
                }
            }
        }
        match stored {
            0 => return Parsed { noun, ..unknown(&w1, table) },
            1 => pos = after_w1,
            _ => {}
        }
    }
    Parsed { verb, noun, unknown: None, cmp_table: table }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Uses the vocabulary extracted from the user's disks; skipped if it isn't there.
    fn vocab() -> Option<Vec<u8>> {
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/extracted/res/fixed/main_0800_vocab.bin")).ok()
    }

    #[test]
    fn parses_against_original_vocabulary() {
        let Some(v) = vocab() else { return };
        let ids = crate::vocab::parse(&v).unwrap();
        let id = |w: &str| ids.iter().find(|e| e.word == w && e.position == 1).map(|e| e.id).unwrap();
        // Verb + ignored words + noun.
        let p = parse(&v, b"look at the castle");
        assert_eq!((p.verb, p.noun, p.unknown.as_deref()), (id("look"), id("castle"), None));
        // Case is folded.
        assert_eq!(parse(&v, b"LOOK Castle").noun, id("castle"));
        // A noun alone parses, with no verb.
        let p = parse(&v, b"castle");
        assert_eq!((p.verb, p.noun), (0, id("castle")));
        // Unknown word.
        let p = parse(&v, b"look xyzzy");
        assert_eq!((p.verb, p.unknown.as_deref()), (0xFF, Some("xyzzy")));
        assert!(p.nothing());
        // Two-word phrase resolves to the id on its last word.
        let phrase = ids.iter().position(|e| e.word == "carrot" && e.position == 2).unwrap();
        let p = parse(&v, b"carrot patch");
        assert_eq!(p.noun, ids[phrase + 1].id);
    }

    /// The byte the compare leaves at $523F, as watched in the original (refemu,
    /// REFEMU_PEEK=523F) after each of these lines.
    #[test]
    fn word_compare_operand_matches_original() {
        let Some(v) = vocab() else { return };
        for (line, t) in [
            ("look", 0x70),
            ("carrot patch", 0x84),
            ("look castle", 0x70),
            ("look carrot", 0x70),
            ("get carrot", 0x70),
            ("xyzzy", 0x70),
            ("look at the castle", 0x70),
            ("carrot", 0x70),
            ("castle look carrot patch", 0x70),
        ] {
            assert_eq!(parse(&v, line.as_bytes()).cmp_table, Some(t), "{line}");
        }
    }
}
