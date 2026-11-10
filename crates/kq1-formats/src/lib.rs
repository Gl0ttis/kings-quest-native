//! The resource formats every port reads: the releases share their data formats, with
//! small differences each port passes in (the PCjr's script operand table, for one).
//! They were first decoded for the Apple IIe, so the notes cite the Apple engine's
//! addresses. No dependencies; no game data.
//!
//! - [`view`]: views (loops and cels);
//! - [`script`]: scripts (the rules and their conditions and actions);
//! - [`vocab`]: the parser's vocabulary;
//! - [`mfm`]: HxC MFM disk images, the preservation dumps of the original disks;
//! - [`png`]: a small PNG writer, for the tools' and players' screenshots.

pub mod mfm;
pub mod png;
pub mod script;
pub mod view;
pub mod vocab;
