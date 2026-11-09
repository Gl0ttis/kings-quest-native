//! Native port of the 1984 IBM PC King's Quest (the CGA release, in composite colour),
//! in progress. So far: the disk image, its files and the resource table
//! (docs/disk-layout.md), and the resources read with the PCjr port's decoders.

pub mod composite;
pub mod disk;
pub mod pic;
pub mod script;
pub mod sound;
pub mod view;
pub mod vocab;
