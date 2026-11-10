//! What the IBM PC and PCjr ports' engines share: the two releases are the same engine
//! at other addresses (ports/pc/docs/engine.md).
//! - [`clock`]: when the timer's ticks and the keys come, and the clocks for replaying
//!   the original's runs and for play;
//! - [`pic`]: the picture painter.

pub mod clock;
pub mod pic;
