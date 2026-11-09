//! Starting, the main loop, the title, and input: the line editor (`$1069`), the
//! wait for a key that runs the game cycle (`$13CE`), and the parser (`$0DC1`).
//!
//! The parser differs from the Apple version's. It is an iterator: each call of
//! `$0DC1` yields one (verb, noun) pair found in the typed line, and the main loop
//! runs a game cycle and the parser rules on it; the next call tries the next pair,
//! until a rule matches or the line has no more, when the next line is read. Its
//! cursors point into the vocabulary or into the typed line, and some of them hold
//! either at different times, as in the original.

use super::World;

/// The vocabulary's end marker.
const VOCAB_END: u8 = b'{';
/// The input line: 37 characters, then 00 FF.
const LINE_MAX: usize = 37;
/// The canned lines typed by keys '3', '5' and Tab (DS:0CDE, 0CE9, 0CF7).
const CANNED: [usize; 3] = [0x0CDE, 0x0CE9, 0x0CF7];
/// "Restart Game (y/n)?" (DS:17A9).
const RESTART_PROMPT: usize = 0x17A9;
/// The silence the music starts with (DS:0D24).
const SILENCE: usize = 0x0D24;

/// A parser cursor: nothing, a place in the vocabulary, or a place in the typed line.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum Ptr {
    #[default]
    Null,
    Vocab(usize),
    Line(usize),
}

impl Ptr {
    fn is_null(self) -> bool {
        self == Ptr::Null
    }
}

/// The parser's and the line editor's state.
#[derive(Clone)]
pub struct Parse {
    /// 0EA9: the first call has been made.
    pub started: bool,
    /// 0E2C: the phrase state.
    pub phrase: u8,
    /// 0EAE: a verb was found in this line.
    pub verb_found: bool,
    /// 0EAF/0EB0: the verb and noun being tried.
    pub verb: u8,
    pub noun: u8,
    /// 0EB1: the line was edited (reprinted with the prompt).
    pub edited: bool,
    /// 0E2A: where the next verb is looked for; 0EB5, 0EB7, 0EB9, 0EBD: the cursors.
    pub next_verb: Ptr,
    pub vocab: Ptr,
    pub noun_at: Ptr,
    pub saved: Ptr,
    pub text: Ptr,
    /// 0FD4/0FD5: the word read.
    pub word: Vec<u8>,
    /// 1000-1002: the vocabulary entry last found: position (2: a phrase's first
    /// word), type (2 verb, 3 noun, 4 ignored), id.
    pub found: [u8; 3],
    /// 0F8A: the typed line (spaces stored as 0), ended by 00 FF.
    pub line: Vec<u8>,
    /// 17A2, 17A4, 17A6: the line's length, the cursor in it, and on screen.
    pub count: u8,
    pub at: usize,
    pub column: u8,
    /// 1F14: a parser rule's actions ran.
    pub acted: bool,
}

impl Default for Parse {
    fn default() -> Self {
        let mut line = vec![0u8; LINE_MAX + 3];
        line[1] = 0xFF;
        Parse {
            started: false,
            phrase: 1,
            verb_found: false,
            verb: 0,
            noun: 0,
            edited: false,
            next_verb: Ptr::Null,
            vocab: Ptr::Null,
            noun_at: Ptr::Null,
            saved: Ptr::Null,
            text: Ptr::Null,
            word: Vec::new(),
            found: [0; 3],
            line,
            count: 0,
            at: 0,
            column: 0,
            acted: false,
        }
    }
}

impl World {
    /// `$0C4C`: start, then the main loop; after a restart, again.
    pub fn start(&mut self) {
        loop {
            // The Play diskette jumps to the restart from deep in a pass (`jmp
            // $0A30`, which resets the stack).
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.game_start();
                self.main_loop();
            }));
            if let Err(e) = r {
                if e.downcast_ref::<super::save::RestartNow>().is_none() {
                    std::panic::resume_unwind(e);
                }
            }
            self.restart();
        }
    }

    /// `$0C5A`: the screen, the title (the first time), the global script, and its
    /// cycle rules once (which go to room 1).
    fn game_start(&mut self) {
        if self.g.first_start {
            // $07C3: mode 9, which clears the pages.
            self.screen.shown.fill(0);
            self.text = super::text::Text::default();
        }
        self.g.text_row = 0x18;
        self.g.text_col = 0;
        self.text.cursor = (0x18, 0);
        self.g.parse.line[0] = 0;
        self.g.parse.line[1] = 0xFF;
        self.g.music_on = false;
        // The music starts with silence.
        self.g.music = Some(super::Stream { data: self.res.ds.clone(), at: SILENCE });
        self.g.music_on = true;
        if self.g.first_start {
            self.g.first_start = false;
            self.timer_on = true;
            self.vars[24] = 3;
            self.title();
        }
        let s = self.load_room_script(0);
        self.global = Some(s.clone());
        self.objects[0].x = 0x78;
        self.objects[0].y = 0x78;
        self.match_cycle_rules(&s);
    }

    /// `$48E8`: the title: the copyright, the Sierra logo (picture 82), then the title
    /// picture (83) with its tune and the credits; a key skips it.
    fn title(&mut self) {
        self.title_clear_rows();
        self.teletype_str(b"\n\n  (c)Copyright Sierra On-Line Inc 1984\r\n      (c)Copyright IBM Corp. 1984");
        self.load_picture(0x52);
        self.g.keys_on = false;
        self.draw_room_picture();
        self.site_keys();
        if self.keys.pop_front().is_none() {
            self.title_wait();
            self.load_sound_group(0x15);
            self.load_picture(0x53);
            self.title_clear_rows();
            self.teletype_str(b"\n\n\n        Press any key to play");
            self.draw_room_picture();
            self.play_sound(0x15);
            let mut row = 7u8;
            for (col, text) in [
                (13, &b"designed and\n"[..]),
                (16, b"written by:\n"),
                (14, b"Roberta Williams\n\n"),
                (13, b"programming:\n"),
                (14, b"Charles Tingley\n"),
                (13, b"Ken MacNeill\n\n"),
                (12, b"artwork:\n"),
                (13, b"Doug MacNeill\n"),
                (12, b"Greg Rowland\n"),
            ] {
                // $4A95: at (row, col), drawn one character at a time.
                let mut c = col;
                self.text.cursor = (row, c);
                for &b in text {
                    if b == b'\n' {
                        row += 1;
                    } else {
                        self.put_char(b);
                        c += 1;
                    }
                    self.text.cursor = (row, c);
                }
            }
            self.wait_until("the tune to end", |w| w.vars[15] != 0);
            self.delay(3 * 0x10000 * 17, false);
            self.play_sound(0x15);
            self.title_wait();
        }
        // $4A5E
        self.title_clear_rows();
        self.text.cursor = (0x18, 0);
        self.g.keys_on = true;
        self.wait_until("the tune to end", |w| w.vars[15] != 0);
    }

    /// `$4AE1`: the text rows 21-24 cleared; the cursor to the start of row 21.
    fn title_clear_rows(&mut self) {
        self.text.scroll(0, 0x15, 0x18);
        self.text.cursor = (0x15, 0);
    }

    /// `$4ACF`: a string through the BIOS teletype.
    fn teletype_str(&mut self, s: &[u8]) {
        for &b in s {
            let (r, c) = self.text.cursor;
            if !matches!(b, 0x0D | 0x0A | 0x08 | 0x07) {
                self.clear_cell(r, c);
            }
            self.text.teletype(b, 7);
        }
    }

    /// INT 10h AH=9 in graphics mode: the character at the cursor. Its cell on the
    /// displayed page is cleared (the glyph would come from the ROM's font; the
    /// characters are kept in [`super::text::Text`]).
    pub fn put_char(&mut self, b: u8) {
        let (r, c) = self.text.cursor;
        self.clear_cell(r, c);
        self.text.put(b);
    }

    fn clear_cell(&mut self, r: u8, c: u8) {
        if r < 21 && c < 40 {
            for y in r as usize * 8..r as usize * 8 + 8 {
                for x in c as usize * 4..c as usize * 4 + 4 {
                    self.screen.shown[y * super::screen::WIDTH + x] = 0;
                }
            }
        }
    }

    /// `$4A7A`: wait for a key, up to about 7 seconds (Esc waits for another key).
    fn title_wait(&mut self) {
        self.checkpoint(0x4A7A);
        self.site_keys();
        if self.keys.is_empty() && !self.delay(30 * 0xF800 * 17, true) {
            return;
        }
        if let Some(k) = self.keys.pop_front() {
            if k as u8 == 0x1B {
                self.wait_until("a key", |w| !w.keys.is_empty());
                self.keys.pop_front();
            }
        }
    }

    /// `$0D16`: the main loop: the next (verb, noun) to try (reading a line when
    /// needed), a game cycle, then the parser rules: the global script's (again while
    /// they ask), the save or restore dialog if asked for, the room's.
    fn main_loop(&mut self) {
        loop {
            if self.g.restoring {
                self.g.key_flags[1] = true;
            }
            self.next_parse();
            if self.g.key_flags[2] {
                return;
            }
            self.game_cycle();
            self.vars[13] = self.vars[13].wrapping_add(1);
            if self.vars[13] == 3 {
                self.vars[13] = 0;
            }
            if let Some(g) = self.global.clone() {
                while self.match_parser_rules(&g) != 0 {}
            }
            if self.g.save_requested || self.g.key_flags[1] {
                if self.g.restoring {
                    self.g.prompt_due = true;
                }
                self.save_restore_dialog();
            }
            if let Some(r) = self.room.clone() {
                self.match_parser_rules(&r);
            }
            if self.g.save_requested || self.g.key_flags[1] {
                self.g.matched = true;
                self.g.key_flags[1] = false;
                self.g.save_requested = false;
            }
        }
    }

    /// `$0DC1`: the next (verb, noun) to try. A line's pairs are tried in turn
    /// (verb alone, or followed by a noun, with ignored words between); when they run
    /// out, or a rule matched, v28 says how the line went (1 no verb, 2 a verb but no
    /// rule, 3 a rule) and the next line is read.
    fn next_parse(&mut self) {
        self.checkpoint(0x0DC1);
        #[derive(PartialEq)]
        enum At {
            NewLine,
            Nouns,
            NextVerb,
            More,
        }
        let mut at = if !self.g.parse.started {
            self.g.parse.started = true;
            At::NewLine
        } else if self.g.parse.phrase != 0 || self.g.matched {
            At::More
        } else {
            At::NextVerb
        };
        loop {
            match at {
                At::More => {
                    let p = &self.g.parse;
                    if !p.next_verb.is_null() && !self.g.matched {
                        at = At::Nouns;
                        continue;
                    }
                    self.vars[28] = if self.g.matched {
                        3
                    } else if self.g.parse.verb_found {
                        2
                    } else {
                        1
                    };
                    at = At::NewLine;
                }
                At::NewLine => {
                    let p = &mut self.g.parse;
                    p.noun_at = Ptr::Null;
                    self.g.prompt_shown = false;
                    p.verb_found = false;
                    self.g.matched = false;
                    p.next_verb = Ptr::Vocab(0);
                    self.read_line();
                    if self.g.key_flags[2] {
                        return;
                    }
                    at = At::Nouns;
                }
                At::Nouns => {
                    if self.g.parse.noun_at.is_null() {
                        at = At::NextVerb;
                        continue;
                    }
                    // $0ED5: past ignored words.
                    loop {
                        let p = &mut self.g.parse;
                        p.saved = p.text;
                        p.vocab = Ptr::Vocab(0);
                        let t = p.text;
                        self.g.parse.text = self.read_word(t);
                        self.vocab_find();
                        if self.g.parse.found[1] != 4 {
                            break;
                        }
                    }
                    loop {
                        let (n, s) = (self.g.parse.noun_at, self.g.parse.saved);
                        let n = self.find_word_of_type(n, s, 3);
                        self.g.parse.noun_at = n;
                        if n.is_null() {
                            break;
                        }
                        self.g.parse.noun = self.g.parse.found[2];
                        let t = self.g.parse.text;
                        if self.read_word(t).is_null() {
                            self.g.parse.verb_found = true;
                            return;
                        }
                        let t = self.g.parse.text;
                        if !self.find_word_of_type(Ptr::Vocab(0), t, 4).is_null() {
                            self.g.parse.verb_found = true;
                            return;
                        }
                    }
                    at = At::NextVerb;
                }
                At::NextVerb => {
                    // $0E44
                    let nv = self.g.parse.next_verb;
                    self.g.parse.saved = nv;
                    let v = self.find_word_of_type(nv, Ptr::Line(0), 2);
                    self.g.parse.next_verb = v;
                    if v.is_null() {
                        at = At::More;
                        continue;
                    }
                    self.g.parse.verb = self.g.parse.found[2];
                    self.g.parse.noun_at = v;
                    let t = self.g.parse.text;
                    if self.read_word(t).is_null() {
                        self.g.parse.noun = 2;
                        self.g.parse.verb_found = true;
                        return;
                    }
                    if self.g.parse.phrase == 1 {
                        loop {
                            let n = self.g.parse.noun_at;
                            let n = self.find_word_of_type(n, Ptr::Line(0), 2);
                            self.g.parse.noun_at = n;
                            if n.is_null() {
                                break;
                            }
                            if self.g.parse.found[0] == 2 {
                                let p = &mut self.g.parse;
                                p.phrase = 0;
                                p.verb = p.found[2];
                                p.next_verb = p.saved;
                                break;
                            }
                        }
                    } else {
                        self.g.parse.phrase = 1;
                    }
                    self.g.parse.noun_at = Ptr::Vocab(0);
                    // $0ED5 again, with the noun search from the vocabulary's start.
                    at = At::Nouns;
                }
            }
        }
    }

    /// `$0F46`: from vocabulary position `vocab`, the next entry of `kind` for the
    /// word at `text`, skipping ignored words (type 4) in the text; a phrase's first
    /// word followed by its second word is taken whatever its type. Null if none.
    fn find_word_of_type(&mut self, mut vocab: Ptr, mut text: Ptr, kind: u8) -> Ptr {
        'word: loop {
            let t = self.read_word(text);
            self.g.parse.text = t;
            if t.is_null() {
                return Ptr::Null;
            }
            loop {
                self.g.parse.vocab = vocab;
                vocab = self.vocab_find();
                if vocab.is_null() {
                    return Ptr::Null;
                }
                let f = self.g.parse.found;
                if f[1] == kind {
                    return vocab;
                }
                if f[1] == 4 {
                    text = self.g.parse.text;
                    vocab = self.g.parse.vocab;
                    continue 'word;
                }
                if f[0] == 2 {
                    break;
                }
            }
            // A phrase: its second word must follow.
            let t = self.g.parse.text;
            let t2 = self.read_word(t);
            self.g.parse.text = t2;
            if !t2.is_null() {
                self.g.parse.vocab = vocab;
                let r = self.vocab_match_second();
                if !r.is_null() {
                    return r;
                }
            }
            // Past the embedded second word and its three bytes; the same word again.
            let Ptr::Vocab(mut i) = self.g.parse.vocab else { return Ptr::Null };
            while self.res.vocab.get(i).copied().unwrap_or(0) != 0 {
                i += 1;
            }
            vocab = Ptr::Vocab(i + 4);
        }
    }

    /// `$0FF6`: the next word of the line from `p` (past zeros, the spaces): it goes to
    /// the word buffer (at most 37 kept). Returns where it ends (its terminating 0), or
    /// null at the line's end.
    fn read_word(&mut self, p: Ptr) -> Ptr {
        let Ptr::Line(mut i) = p else { return Ptr::Null };
        let line = &self.g.parse.line;
        while line.get(i).copied().unwrap_or(0xFF) == 0 {
            i += 1;
        }
        if line.get(i).copied().unwrap_or(0xFF) == 0xFF {
            return Ptr::Null;
        }
        let mut w = Vec::new();
        while let Some(&c) = line.get(i) {
            if c == 0 {
                break;
            }
            if w.len() < LINE_MAX {
                w.push(c);
            }
            i += 1;
        }
        self.g.parse.word = w;
        Ptr::Line(i)
    }

    /// `$0BA6`: the word read, in the vocabulary from the cursor: the entries are
    /// sorted, and the search stops at the first whose first letter is past the
    /// word's. The entry's 3 bytes (position, type, id) are kept; returns where they
    /// end, or null (and zeros) if not found.
    fn vocab_find(&mut self) -> Ptr {
        let Ptr::Vocab(mut di) = self.g.parse.vocab else { return Ptr::Null };
        let v = &self.res.vocab;
        let w = &self.g.parse.word;
        let at = |i: usize| v.get(i).copied().unwrap_or(VOCAB_END);
        loop {
            if at(di) == VOCAB_END {
                break;
            }
            let first = w.first().copied().unwrap_or(0);
            if first < at(di) {
                break;
            }
            let mut k = di;
            let mut same = first == at(di);
            k += 1;
            if same {
                for &c in w.iter().skip(1) {
                    let e = at(k);
                    k += 1;
                    if c != e {
                        same = false;
                        break;
                    }
                }
                if same && at(k) == 0 {
                    let f = [at(k + 1), at(k + 2), at(k + 3)];
                    self.g.parse.found = f;
                    return Ptr::Vocab(k + 4);
                }
            }
            // Past this entry: from the mismatch to its 0, its 3 bytes; a phrase's
            // first word also holds the second word and 3 more.
            let mut j = k - 1;
            while at(j) != 0 {
                j += 1;
            }
            j += 1;
            if at(j) == 1 {
                j += 3;
            } else {
                j += 3;
                while at(j) != 0 {
                    j += 1;
                }
                j += 1 + 3;
            }
            di = j;
        }
        self.g.parse.found = [0; 3];
        Ptr::Null
    }

    /// `$0BFC`: the word read as a phrase's second word, at the cursor: its 3 bytes
    /// kept, returns where they end; null if it isn't.
    fn vocab_match_second(&mut self) -> Ptr {
        let Ptr::Vocab(di) = self.g.parse.vocab else { return Ptr::Null };
        let v = &self.res.vocab;
        let at = |i: usize| v.get(i).copied().unwrap_or(VOCAB_END);
        let w = &self.g.parse.word;
        if w.iter().enumerate().any(|(i, &c)| at(di + i) != c) || at(di + w.len()) != 0 {
            return Ptr::Null;
        }
        let k = di + w.len();
        self.g.parse.found = [at(k + 1), at(k + 2), at(k + 3)];
        Ptr::Vocab(k + 4)
    }

    /// `$1069`: read a line: keys from the ring go into it and onto row 24; returns on
    /// Enter with something typed (or when restarting).
    fn read_line(&mut self) {
        self.checkpoint(0x1069);
        loop {
            let mut bs = false;
            let mut key = self.wait_key();
            if self.g.key_flags[2] {
                return;
            }
            if self.g.parse.acted && self.vars[23] == 0 {
                self.g.parse.acted = false;
                self.g.prompt_due = true;
            }
            if self.g.prompt_due && !self.g.restoring {
                if self.vars[23] == 0 {
                    // The prompt, and the line typed so far if it was edited.
                    self.g.text_col = 0;
                    self.text.cursor = (0x18, 0);
                    self.put_char(b'>');
                    let old = self.g.parse.count;
                    self.g.prompt_due = false;
                    let p = &mut self.g.parse;
                    p.count = 0;
                    p.column = 2;
                    p.at = 0;
                    if p.edited && old != 0 {
                        for _ in 0..old {
                            let p = &self.g.parse;
                            let c = match p.line[p.at] {
                                0 => b' ',
                                c => c,
                            };
                            self.text.cursor = (0x18, p.column);
                            self.put_char(c);
                            let p = &mut self.g.parse;
                            p.at += 1;
                            p.count += 1;
                            p.column += 1;
                        }
                    }
                }
                continue;
            }
            let f = self.g.key_flags;
            if f[0] || f[1] || f[4] {
                // '3', '5' or Tab: "save game", "restore game" or "show status".
                let which = if f[4] {
                    2
                } else if f[1] {
                    1
                } else {
                    0
                };
                let text: Vec<u8> = self.res.ds[CANNED[which]..].iter().take_while(|&&b| b != 0xFF).copied().collect();
                let p = &mut self.g.parse;
                p.line[..text.len()].copy_from_slice(&text);
                p.line[text.len()] = 0xFF;
                p.at = text.len();
                self.wait_print_done();
                self.g.key_flags[3] = true;
            }
            if self.g.key_flags[3] && self.vars[23] == 0 {
                // Retype the line ('9', or a canned line).
                self.g.key_flags[3] = false;
                let p = &mut self.g.parse;
                p.count = 0;
                p.column = 2;
                p.edited = true;
                p.at = 0;
                loop {
                    let p = &self.g.parse;
                    let c = p.line[p.at];
                    if c == 0xFF {
                        break;
                    }
                    if !self.g.restoring {
                        self.text.cursor = (0x18, p.column);
                        self.put_char(if c == 0 { b' ' } else { c });
                    }
                    let p = &mut self.g.parse;
                    p.count += 1;
                    p.column += 1;
                    p.at += 1;
                }
                let f = self.g.key_flags;
                if !f[0] && !f[1] && !f[4] {
                    key = 0x0E;
                } else {
                    key = 0x0D;
                    self.g.key_flags[0] = false;
                }
            }
            if self.g.parse.count as usize == LINE_MAX && key != 0x0E {
                key = 0x0D;
            }
            match key {
                0x20 => {
                    let p = &mut self.g.parse;
                    p.line[p.at] = 0;
                    p.edited = true;
                }
                0x0E => {
                    let p = &mut self.g.parse;
                    if p.count != 0 {
                        p.at -= 1;
                        p.count -= 1;
                        p.column -= 1;
                    }
                    bs = true;
                    key = b' ';
                }
                0x0D => {
                    let p = &mut self.g.parse;
                    p.line[p.at] = 0;
                    p.line[p.at + 1] = 0xFF;
                    if !self.g.restoring {
                        self.text_newline();
                    }
                    let p = &mut self.g.parse;
                    p.at = 0;
                    while p.line[p.at] == 0 {
                        p.at += 1;
                    }
                    p.edited = false;
                    if p.line[p.at] == 0xFF {
                        self.g.prompt_due = true;
                        continue;
                    }
                    return;
                }
                b'+' => {
                    // The next character of the line before.
                    let p = &self.g.parse;
                    self.text.cursor = (0x18, p.column);
                    let c = p.line[p.at];
                    if c == 0xFF {
                        continue;
                    }
                    self.put_char(if c == 0 { b' ' } else { c });
                    let p = &mut self.g.parse;
                    p.at += 1;
                    p.count += 1;
                    p.column += 1;
                    p.edited = true;
                    continue;
                }
                k => {
                    let p = &mut self.g.parse;
                    p.line[p.at] = k;
                    p.edited = true;
                }
            }
            // $138D: the character (or the space a backspace leaves) on row 24.
            let col = self.g.parse.column;
            self.text.cursor = (0x18, col);
            self.put_char(key);
            if bs || self.g.parse.count as usize >= LINE_MAX {
                continue;
            }
            let p = &mut self.g.parse;
            p.at += 1;
            p.count += 1;
            p.column += 1;
        }
    }

    /// `$13CE`: run game cycles until the line editor has something to do: a key from
    /// the ring, the prompt to draw, a canned line or a retype; or "y" to the restart
    /// question. The timers v2 and v9-v12 count down once a loop. Returns the key.
    fn wait_key(&mut self) -> u8 {
        self.checkpoint(0x13CE);
        loop {
            self.game_cycle();
            if self.g.key_flags[1] || self.g.key_flags[4] {
                return 0;
            }
            self.wait_until("the pause to end", |w| !w.g.paused);
            if self.g.key_flags[2] {
                if self.g.keys_on {
                    self.g.keys_on = false;
                    self.wait_print_done();
                    self.g.print_all = true;
                    self.queue_engine_string(RESTART_PROMPT);
                    self.wait_print_done();
                    self.g.print_all = false;
                    continue;
                }
                match self.poll_key_bios() as u8 {
                    b'y' => return b'y',
                    b'n' => {
                        self.g.key_flags[2] = false;
                        self.g.keys_on = true;
                    }
                    _ => continue,
                }
            }
            if self.vars[2] != 0 {
                self.vars[2] -= 1;
            }
            for v in 9..13 {
                if self.vars[v] != 0 {
                    self.vars[v] -= 1;
                }
            }
            self.site(super::clock::Site::WaitKeyTimers);
            if self.g.input_off {
                self.g.play_disk = false;
                self.g.key_flags[3] = false;
                self.g.key_flags[0] = false;
                continue;
            }
            let f = self.g.key_flags;
            if f[0] || self.g.prompt_due || f[3] && self.vars[23] == 0 {
                return 0;
            }
            if self.vars[23] == 0 {
                if !self.g.prompt_shown {
                    self.g.prompt_due = true;
                    self.g.prompt_shown = true;
                    return 0;
                }
                let k = self.ring_pop();
                if k != 0 {
                    return k;
                }
            } else {
                self.g.prompt_shown = false;
            }
            if self.g.play_disk {
                self.g.play_disk = false;
                self.make_play_diskette();
            }
        }
    }

    /// `$1A6C`: the next key from the ring (0 if none).
    fn ring_pop(&mut self) -> u8 {
        self.checkpoint(0x1A6C);
        self.g.ring.pop_front().unwrap_or(0)
    }

    /// `$0B92`: a key straight from the BIOS, bypassing the ring: its character, 100h
    /// for a key without one, 0 if none is waiting.
    pub fn poll_key_bios(&mut self) -> u16 {
        self.checkpoint(0x0B92);
        self.site_keys();
        match self.keys.pop_front() {
            None => 0,
            Some(k) if k as u8 == 0 => 0x100,
            Some(k) => k as u8 as u16,
        }
    }
}
