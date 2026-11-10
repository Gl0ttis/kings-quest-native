//! The resource table's entries, the same on the IBM PC and PCjr disks.

/// A sector's bytes.
pub const SECTOR: usize = 512;

/// A directory entry: the resource starts `offset` bytes into `sector` and is read as
/// `sectors` sectors from there (the PCjr engine's `$4B35`, `$4BBF`, `$4D06`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Entry {
    pub sector: usize,
    pub offset: usize,
    pub sectors: usize,
}

impl Entry {
    /// Decode the 4 bytes `b0 b1 b2 b3`: offset = b0 | (b1 bit 7) << 8; sectors =
    /// (b1:b2 >> 4) & 0x3FF; sector = (b2:b3) & 0x3FF (big-endian pairs). `None` for an
    /// unused entry, which each disk fills with its own `unused` bytes (`****` on the
    /// PCjr's, zeros on the PC's).
    pub fn decode(b: [u8; 4], unused: [u8; 4]) -> Option<Entry> {
        if b == unused {
            return None;
        }
        Some(Entry {
            offset: b[0] as usize | ((b[1] >> 7) as usize) << 8,
            sectors: (u16::from_be_bytes([b[1], b[2]]) >> 4) as usize & 0x3FF,
            sector: u16::from_be_bytes([b[2], b[3]]) as usize & 0x3FF,
        })
    }

    /// Where the resource starts in the image.
    pub fn start(&self) -> usize {
        self.sector * SECTOR + self.offset
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries() {
        assert_eq!(
            Entry::decode([0x25, 0x80, 0x40, 0x6a], *b"****"),
            Some(Entry { sector: 106, offset: 293, sectors: 4 })
        );
        assert_eq!(
            Entry::decode([0x18, 0x80, 0x80, 0xa6], [0; 4]),
            Some(Entry { sector: 166, offset: 280, sectors: 8 })
        );
        assert_eq!(Entry::decode(*b"****", *b"****"), None);
        assert_eq!(Entry::decode([0; 4], [0; 4]), None);
    }
}
