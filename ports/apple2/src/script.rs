//! Room/global script ("logic") resources: structure and static parsing.
//! Engine addresses refer to the main 6502 engine image at $4000.
//!
//! A script has an 8-byte header of four u16 section *lengths*. Section 0 starts at
//! offset 8 and each following section starts where the previous one ends ($42E7-$4317):
//!   0: action code   -- run by the action interpreter ($58AF)
//!   1: cycle rules   -- matched every game cycle ($40BA, matcher mode 1)
//!   2: parser rules  -- matched after the player enters a command ($408E, mode 0)
//!   3: messages      -- NUL-terminated text, ended by $FF
//! Script 0 is the global script; it is loaded once and room scripts load after it.

use std::fmt;

/// Number of operand bytes fetched by each action opcode $00-$43 (dispatch table $6AF3).
/// Derived by exploring every control-flow path of each handler for calls to the
/// operand fetch ($58F2), and confirmed by parsing all 81 scripts: every section-0
/// stream splits exactly into $FF-terminated sequences, and every action entry used by
/// a rule lands on an opcode boundary (one harmless exception, see docs/script-format.md).
pub const OPERANDS: [u8; 0x44] = [
    0, 0, 1, 0, 1, 0, 0, 1, 1, 1, 2, 1, 1, 1, 2, 1, // 00-0F
    1, 1, 2, 2, 1, 1, 1, 1, 3, 3, 1, 1, 1, 1, 2, 1, // 10-1F
    3, 3, 1, 1, 1, 0, 3, 3, 1, 1, 0, 0, 1, 2, 0, 2, // 20-2F
    2, 2, 0, 0, 1, 1, 2, 1, 2, 1, 1, 1, 2, 4, 2, 0, // 30-3F
    1, 1, 0, 0, // 40-43
];

/// Opcode names. Each is based on what the handler does to engine state (see
/// docs/script-format.md for the field-level effect and evidence). Names that read as
/// mechanics, such as `obj_flag0_on`, are handlers whose game-level purpose is not yet known.
/// `None` = not yet characterised.
pub fn op_name(op: u8) -> Option<&'static str> {
    Some(match op {
        0x00 => "nop",
        0x01 => "reset_ego",
        0x02 => "add_actor",
        0x03 => "clear_actors",
        0x04 => "new_room",
        0x05 => "redraw_room",
        0x06 => "show_overlay_picture",
        0x07 => "draw",
        0x08 => "set",
        0x09 => "reset",
        0x0A => "print",
        0x0B => "carry",
        0x0C => "obj_erase",
        0x0D => "uncarry",
        0x0E => "copy_position",
        0x0F => "input_number",
        0x10 => "increment",
        0x11 => "decrement",
        0x12 => "assign",
        0x13 => "set_cel",
        0x14 => "add_score",
        0x15 => "sub_score",
        0x16 => "play_loop_once",
        0x17 => "play_loop_reverse",
        0x18 => "move_to",
        0x19 => "position",
        0x1A => "obj_field4_on",
        0x1B => "obj_field4_off",
        0x1C => "set_ego_view",
        0x1D => "approach_ego",
        0x1E => "follow_ego",
        0x1F => "chase_ego",
        0x20 => "set_x_limits",
        0x21 => "set_y_limits",
        0x22 => "wander",
        0x23 => "ego_move_to_y",
        0x24 => "load_view",
        0x25 => "redraw_picture",
        0x26 => "place_object",
        0x27 => "set_loop_cel",
        0x28 => "ego_move_to_x",
        0x29 => "nop1",
        0x2A => "program_control",
        0x2B => "disable_input",
        0x2C => "load_picture",
        0x2D => "ego_special_view",
        0x2E => "player_control",
        0x2F => "set_priority",
        0x30 => "set_view",
        0x31 => "inventory_screen",
        0x32 => "save_game",
        0x33 => "restore_game",
        0x34 => "release_priority",
        0x35 => "copy_room_to",
        0x36 => "set_if_room_eq",
        0x37 => "new_room_var",
        0x38 => "set_loop",
        0x39 => "store_position",
        0x3A => "obj_flags6_on",
        0x3B => "obj_flags6_off",
        0x3C => "text_screen",
        0x3D => "print_at",
        0x3E => "print_text",
        0x3F => "graphics_screen",
        0x40 => "obj_flag8_on",
        0x41 => "set_ego_view",
        0x42 => "init_save_disk",
        0x43 => "restart_prompt",
        _ => return None,
    })
}

/// For opcodes that print a message: the index of the first of two operand bytes that
/// form a u16 offset into the running script's message section ($576C).
pub fn message_operand(op: u8) -> Option<usize> {
    match op {
        0x0A | 0x3E => Some(0),
        0x3D => Some(2),
        _ => None,
    }
}

#[derive(Debug, Clone)]
pub struct Instr {
    pub offset: usize, // within section 0
    pub op: u8,
    pub args: Vec<u8>,
}

impl fmt::Display for Instr {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match op_name(self.op) {
            Some(n) => write!(f, "{n}")?,
            None => write!(f, "op_{:02X}", self.op)?,
        }
        for a in &self.args {
            write!(f, " {a}")?;
        }
        Ok(())
    }
}

/// A (variable, value) test; bit 7 of the stored value means "not equal" ($638D).
#[derive(Debug, Clone, Copy)]
pub struct Cond {
    pub var: u8,
    pub value: u8,
    pub negate: bool,
}

impl fmt::Display for Cond {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "v{}{}{}", self.var, if self.negate { "!=" } else { "==" }, self.value)
    }
}

#[derive(Debug, Clone)]
pub enum RuleKind {
    /// $FF: end of rule list.
    End,
    /// $FE off: continue at `target`.
    Goto { target: usize },
    /// $FC (or $FB in parser mode) conds $FE off: if conditions fail, continue at `target`.
    If { conds: Vec<Cond>, target: usize },
    /// Parser mode $FD x1 y1 x2 y2 conds $FE off: ego inside rect and conds, else `target`.
    IfEgoIn { rect: [u8; 4], conds: Vec<Cond>, target: usize },
    /// Parser mode: verb noun conds $FE act. Noun 1 matches any noun.
    Said { verb: u8, noun: u8, conds: Vec<Cond>, action: usize },
    /// Cycle mode $FB conds $FE act.
    Always { conds: Vec<Cond>, action: usize },
    /// Cycle mode x1 y1 x2 y2 conds $FE act: ego inside rect ($6363).
    EgoIn { rect: [u8; 4], conds: Vec<Cond>, action: usize },
}

#[derive(Debug, Clone)]
pub struct Rule {
    pub offset: usize, // within its section
    pub kind: RuleKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Cycle,
    Parser,
}

pub struct Script<'a> {
    pub data: &'a [u8],
    /// Start offsets of sections 0..3 and the end offset declared by the header.
    pub bounds: [usize; 5],
}

impl<'a> Script<'a> {
    pub fn new(data: &'a [u8]) -> Result<Self, String> {
        if data.len() < 8 {
            return Err("script shorter than header".into());
        }
        let mut bounds = [8usize; 5];
        for i in 0..4 {
            bounds[i + 1] = bounds[i] + u16::from_le_bytes([data[i * 2], data[i * 2 + 1]]) as usize;
        }
        Ok(Script { data, bounds })
    }

    pub fn section(&self, n: usize) -> &'a [u8] {
        let end = self.bounds[n + 1].min(self.data.len());
        &self.data[self.bounds[n].min(end)..end]
    }

    /// Parse section 0 into $FF-terminated sequences, keyed by start offset.
    pub fn actions(&self) -> Result<Vec<(usize, Vec<Instr>)>, String> {
        let s = self.section(0);
        let mut out = Vec::new();
        let mut pos = 0;
        while pos < s.len() {
            let start = pos;
            let mut seq = Vec::new();
            loop {
                let op = *s.get(pos).ok_or(format!("action at {start} runs off section"))?;
                pos += 1;
                if op == 0xFF {
                    break;
                }
                let n = *OPERANDS.get(op as usize).ok_or(format!("bad opcode ${op:02X} at {}", pos - 1))? as usize;
                let args = s.get(pos..pos + n).ok_or(format!("operands of ${op:02X} run off section"))?.to_vec();
                seq.push(Instr { offset: pos - 1, op, args });
                pos += n;
            }
            out.push((start, seq));
        }
        Ok(out)
    }

    /// Structural parse of a rule section (1 = cycle, 2 = parser). Targets are section offsets.
    pub fn rules(&self, mode: Mode) -> Result<Vec<Rule>, String> {
        let s = self.section(if mode == Mode::Cycle { 1 } else { 2 });
        let mut p = Reader { s, pos: 0 };
        let mut out = Vec::new();
        while p.pos < s.len() {
            let offset = p.pos;
            let op = p.byte()?;
            let kind = match (op, mode) {
                (0xFF, _) => RuleKind::End,
                (0xFE, _) => RuleKind::Goto { target: p.rel()? },
                (0xFC, _) | (0xFB, Mode::Parser) => {
                    let conds = p.conds()?;
                    RuleKind::If { conds, target: p.rel()? }
                }
                (0xFB, Mode::Cycle) => {
                    let conds = p.conds()?;
                    RuleKind::Always { conds, action: p.word()? }
                }
                (0xFD, Mode::Parser) => {
                    let rect = p.rect(None)?;
                    let conds = p.conds()?;
                    RuleKind::IfEgoIn { rect, conds, target: p.rel()? }
                }
                (verb, Mode::Parser) => {
                    let noun = p.byte()?;
                    let conds = p.conds()?;
                    RuleKind::Said { verb, noun, conds, action: p.word()? }
                }
                (x1, Mode::Cycle) => {
                    let rect = p.rect(Some(x1))?;
                    let conds = p.conds()?;
                    RuleKind::EgoIn { rect, conds, action: p.word()? }
                }
            };
            out.push(Rule { offset, kind });
        }
        Ok(out)
    }

    /// The message starting at byte `offset` of section 3, if `offset` is the start of one.
    pub fn message_at(&self, offset: usize) -> Option<String> {
        let s = self.section(3);
        if offset >= s.len() || (offset > 0 && s[offset - 1] != 0) {
            return None;
        }
        let end = s[offset..].iter().position(|&b| b == 0 || b == 0xFF).map_or(s.len(), |e| offset + e);
        Some(s[offset..end].iter().map(|&c| (c & 0x7F) as char).collect())
    }

    /// Messages in section 3, in order.
    pub fn messages(&self) -> Vec<String> {
        let s = self.section(3);
        let s = match s.iter().rposition(|&b| b == 0xFF) {
            Some(i) => &s[..i],
            None => s,
        };
        let mut v: Vec<String> =
            s.split(|&b| b == 0).map(|m| m.iter().map(|&c| (c & 0x7F) as char).collect()).collect();
        if v.last().is_some_and(|m| m.is_empty()) {
            v.pop();
        }
        v
    }
}

struct Reader<'a> {
    s: &'a [u8],
    pos: usize,
}

impl Reader<'_> {
    fn byte(&mut self) -> Result<u8, String> {
        let b = *self.s.get(self.pos).ok_or_else(|| format!("rule runs off section at {}", self.pos))?;
        self.pos += 1;
        Ok(b)
    }
    fn word(&mut self) -> Result<usize, String> {
        Ok(self.byte()? as usize | (self.byte()? as usize) << 8)
    }
    /// Offsets are relative to the byte after the offset itself ($62D0).
    fn rel(&mut self) -> Result<usize, String> {
        let o = self.word()?;
        Ok((self.pos + o) & 0xFFFF)
    }
    fn rect(&mut self, first: Option<u8>) -> Result<[u8; 4], String> {
        let a = match first {
            Some(b) => b,
            None => self.byte()?,
        };
        Ok([a, self.byte()?, self.byte()?, self.byte()?])
    }
    fn conds(&mut self) -> Result<Vec<Cond>, String> {
        let mut c = Vec::new();
        loop {
            let var = self.byte()?;
            if var == 0xFE {
                return Ok(c);
            }
            let v = self.byte()?;
            c.push(Cond { var, value: v & 0x7F, negate: v & 0x80 != 0 });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_and_grammar() {
        // header: sect0 len 4, sect1 len 1, sect2 len 9, sect3 len 4
        let mut d = vec![4, 0, 1, 0, 9, 0, 4, 0];
        d.extend([0x08, 5, 0xFF, 0xFF]); // action 0: set v5; action 3: empty
        d.extend([0xFF]); // cycle rules: end
        d.extend([0x10, 0x01, 0x05, 0x81, 0xFE, 0x00, 0x00, 0xFF, 0xFF]); // said 16 any v5!=1 -> act 0
        d.extend(*b"Hi\0\xFF");
        let s = Script::new(&d).unwrap();
        let acts = s.actions().unwrap();
        assert_eq!(acts.len(), 2);
        assert_eq!(acts[0].1[0].to_string(), "set 5");
        let r = s.rules(Mode::Parser).unwrap();
        match &r[0].kind {
            RuleKind::Said { verb: 0x10, noun: 1, conds, action: 0 } => assert!(conds[0].negate),
            k => panic!("{k:?}"),
        }
        assert_eq!(s.messages(), vec!["Hi"]);
    }
}
