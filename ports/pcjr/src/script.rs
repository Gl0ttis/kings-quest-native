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
