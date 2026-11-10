//! The picture painter, shared with the IBM PC port (crates/kq1-ibm), and where the
//! PCjr engine keeps the frame it draws round every room picture.

pub use kq1_ibm::pic::*;

/// The frame the engine draws before every room picture (`$34FF`: the sequence at
/// `DS:$1981`, in the engine's data segment): a black, priority-0 rectangle round the
/// edge. Fills stop at it.
pub const FRAME: usize = 0x1981;
