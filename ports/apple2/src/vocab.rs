//! Parser vocabulary: fixed resource `main_0800_vocab` (loaded to main $0800).
//!
//! Entries are a NUL-terminated lowercase word followed by (position, type, id):
//!   position 1 = a single word; 2 = first word of a phrase; 3 = a following phrase word
//!   type 2 = verb, 3 = noun, 4 = ignored word, 0 = phrase start (type given on the last word)
//! The list ends with an empty word. Multi-word phrases ("carrot patch") resolve to the id
//! on their final word. Parser rules compare verb and noun ids against $9A/$9B; noun id 1
//! in a rule matches any noun.

#[derive(Debug, Clone)]
pub struct Entry {
    pub word: String,
    pub position: u8,
    pub kind: u8,
    pub id: u8,
}

pub fn parse(data: &[u8]) -> Result<Vec<Entry>, String> {
    let mut out = Vec::new();
    let mut pos = 0;
    loop {
        let end = data[pos..].iter().position(|&b| b == 0).ok_or("unterminated word")? + pos;
        if end == pos {
            return Ok(out);
        }
        let meta = data.get(end + 1..end + 4).ok_or("truncated entry")?;
        out.push(Entry {
            word: data[pos..end].iter().map(|&c| c as char).collect(),
            position: meta[0],
            kind: meta[1],
            id: meta[2],
        });
        pos = end + 4;
    }
}

/// Lookup of display names for verb and noun ids (phrases are joined with spaces).
pub struct Names {
    pub verbs: Vec<Vec<String>>,
    pub nouns: Vec<Vec<String>>,
}

impl Names {
    pub fn new(entries: &[Entry]) -> Self {
        let mut n = Names { verbs: vec![Vec::new(); 256], nouns: vec![Vec::new(); 256] };
        let mut phrase: Vec<&str> = Vec::new();
        for e in entries {
            match e.position {
                2 => {
                    phrase = vec![&e.word];
                    continue;
                }
                3 => phrase.push(&e.word),
                _ => phrase = vec![&e.word],
            }
            let text = phrase.join(" ");
            let list = match e.kind {
                2 => &mut n.verbs[e.id as usize],
                3 => &mut n.nouns[e.id as usize],
                _ => continue,
            };
            if !list.contains(&text) {
                list.push(text);
            }
        }
        n
    }

    fn show(list: &[String], id: u8) -> String {
        match list.first() {
            Some(w) => w.clone(),
            None => format!("#{id}"),
        }
    }
    pub fn verb(&self, id: u8) -> String {
        Self::show(&self.verbs[id as usize], id)
    }
    pub fn noun(&self, id: u8) -> String {
        if id == 1 {
            return "*".into();
        }
        Self::show(&self.nouns[id as usize], id)
    }
}
