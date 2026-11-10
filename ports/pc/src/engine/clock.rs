//! When the timer interrupt fires.
//!
//! The timer interrupt (INT 08h, chained to the game's INT 1Ch handler `$1A04`) comes
//! 18.2 times a second, between any two instructions. The handler reads the keys into
//! the game's key ring, prints queued messages a word at a time, plays the music and
//! ends the pass's wait (`$3FE7`), so the game's state depends on where in its code each
//! tick falls.
//!
//! **Checkpoints.** Both sides count the checkpoints passed, in order: the routine
//! entries and loop heads in [`CHECKPOINTS`]. refemu-pc (`--events`) logs each tick,
//! key press and disk swap with the count at that moment, and for a tick the address
//! it interrupted.
//!
//! **Where in an interval.** Between two checkpoints, the port fires a tick:
//! - at a disk read, if the tick interrupted the read ([`Site::Disk`]);
//! - at the start of the game cycle, before it reads the keypad's direction
//!   ([`Site::CycleStart`]);
//! - before an action that writes a variable, if the tick interrupted its dispatch or
//!   its handler, which writes last ([`Site::Op`]): `set 25` when a room starts lets
//!   the timer read the keys, so a tick during it must not;
//! - in the draw pass, before it sets the flag the next pass waits on, if the tick
//!   interrupted it ([`Site::DrawPass`]);
//! - in a busy wait (a loop on a flag the timer changes), one by one until the wait
//!   is over;
//! - otherwise at the next checkpoint, after the code between.
//!
//! Events keep their order. So the port's logic can be compared with the original's
//! exactly, whatever the timing. For play, [`Paced`] makes the ticks from the port's
//! own estimate of the original's speed.

pub use kq1_ibm::clock::*;

/// Code-segment addresses that are checkpoints: routine entries, and the heads of the
/// rule and action loops (`$115A`, `$2F03`), whose visits the port repeats exactly.
/// None is the head of a busy wait (as `$0F91` and `$0B14` are), whose count of turns
/// the port can't know; `$0785` loops once per key read, which it can.
pub use crate::checkpoints::CHECKPOINTS;

/// Where in an interval the port fires the ticks that interrupted that code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Site {
    /// A disk read (`$1FB6`-`$2093`).
    Disk,
    /// The game cycle's start (`$293A`-`$2946`), before it reads the keypad's direction
    /// (DS:187F) at `$2946`: a key the timer takes there steers this pass. (A tick's
    /// address is the instruction it interrupted, still to run: at `$2946`, the read.)
    CycleStart,
    /// Action `n`'s dispatch (`$2F03`-`$2F1B`, `$38C1`-`$38D3`) and its handler, up to
    /// the jump back to `$2F03`, for the actions that write a variable (08-12, 35, 36:
    /// set, reset, increment, decrement, assign, and the room tests); and for print (0A),
    /// up to its first message queued (`$2FEC`-`$2FF5`, the queueing itself below).
    Op(u8),
    /// Queueing a message (`$1C72`-`$1C88`, up to its `cli`): a tick there finds the
    /// queue as it was, which the timer's print countdown runs on only when it isn't
    /// empty.
    QueueAdd,
    /// The draw pass (`$3FB7`, from its checkpoint) and the routines it calls, up to
    /// where it sets the flag the next pass's wait waits on to clear (`$3FFD`): every
    /// address but the caller's code after it returns (`$2A59`-`$2A83`) and the next
    /// checkpoint's (`$1150`). A tick there comes before the flag is set, so the next
    /// pass waits.
    DrawPass,
    /// wait_key's timer countdown (v2, v9-v12) and its tests up to v23's (`$288D`-
    /// `$28E8`): a tick there that prints the last queued message (v23 = 0) lets this
    /// loop return to the line editor.
    WaitKeyTimers,
}

impl Site {
    pub fn contains(self, ip: u16) -> bool {
        match self {
            Site::Disk => (0x1FB6..0x2094).contains(&ip),
            Site::CycleStart => (0x293A..=0x2946).contains(&ip),
            Site::WaitKeyTimers => (0x288D..=0x28E8).contains(&ip),
            Site::QueueAdd => (0x1C72..=0x1C88).contains(&ip),
            Site::DrawPass => !(0x2A59..=0x2A83).contains(&ip) && ip != 0x1150,
            Site::Op(op) => {
                let handler = match op {
                    0x0A => 0x2FEC..0x2FF6,
                    0x08 => 0x2FC4..0x2FD5,
                    0x09 => 0x2FD8..0x2FE9,
                    0x10 => 0x30E9..0x310F,
                    0x11 => 0x3112..0x3136,
                    0x12 => 0x3139..0x3158,
                    0x35 => 0x3656..0x3669,
                    0x36 => 0x366C..0x369A,
                    _ => return false,
                };
                (0x2F03..0x2F1C).contains(&ip) || (0x38C1..0x38D4).contains(&ip) || handler.contains(&ip)
            }
        }
    }
}
