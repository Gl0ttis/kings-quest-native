//! The PCjr scripts: the Apple version's layout (ports/apple2/src/script.rs), with the
//! PCjr engine's operand counts.

/// Operand bytes fetched by each action opcode `$00`-`$43`. The action interpreter
/// (engine `$1ABB`) reads an opcode, and `$2479` jumps through the table at `$23F3`
/// with opcode − 1: `$01`-`$43` have handlers; `$00` does nothing; `$FF` ends the
/// sequence. Each handler fetches its operands from the script pointer at `[bp+6]`
/// itself or through a helper. The counts match the Apple version's except:
/// - `$05` takes 1 (the room number, read by the loader `$4B35`); the Apple's 0;
/// - `$25` takes 1 (`$201F` → `$44FE`); the Apple's 0;
/// - `$31` takes none (`$21BD`); the Apple's 2.
pub const OPERANDS: [u8; 0x44] = {
    let mut t = kq1_apple2::script::OPERANDS;
    t[0x05] = 1;
    t[0x25] = 1;
    t[0x31] = 0;
    t
};

/// Room `room`'s script as text: the cycle rules, the parser rules (the PCjr's words
/// named from its vocabulary) and the actions, one per line (as the Apple port's
/// kqscript writes them).
pub fn listing(disk: &crate::disk::Disk, room: usize) -> Option<String> {
    use kq1_apple2::script::{Cond, Mode, RuleKind, Script};
    use std::fmt::Write as _;
    let words = crate::vocab::read(&disk.code_segment()).ok()?;
    let names = kq1_apple2::vocab::Names::new(&words);
    let r = disk.room(room)?;
    let s = Script::new(r.script).ok()?;
    let mut t = String::new();
    let cs = |c: &[Cond]| c.iter().map(|c| c.to_string()).collect::<Vec<_>>().join(" && ");
    for (mode, title) in [(Mode::Cycle, "CYCLE RULES"), (Mode::Parser, "PARSER RULES")] {
        writeln!(t, "== {title}").ok()?;
        for rule in s.rules(mode).unwrap_or_default() {
            let line = match &rule.kind {
                RuleKind::End => "end".to_string(),
                RuleKind::Goto { target } => format!("goto {target:04X}"),
                RuleKind::If { conds, target } => format!("if {} else goto {target:04X}", cs(conds)),
                RuleKind::IfEgoIn { rect, conds, target } => {
                    format!("if ego in {rect:?} {} else goto {target:04X}", cs(conds))
                }
                RuleKind::Said { verb, noun, conds, action } => {
                    format!("said({}, {}) {} -> A{action:04X}", names.verb(*verb), names.noun(*noun), cs(conds))
                }
                RuleKind::Always { conds, action } => format!("when {} -> A{action:04X}", cs(conds)),
                RuleKind::EgoIn { rect, conds, action } => format!("ego in {rect:?} {} -> A{action:04X}", cs(conds)),
            };
            writeln!(t, "  {:04X}  {line}", rule.offset).ok()?;
        }
    }
    writeln!(t, "== ACTIONS").ok()?;
    for (start, seq) in s.actions_with(&OPERANDS).unwrap_or_default() {
        writeln!(t, "  A{start:04X}:").ok()?;
        for i in seq {
            writeln!(t, "    {:04X}  {i}", i.offset).ok()?;
        }
    }
    Some(t)
}
