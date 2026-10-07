//! `.pk3` archives: plain zip files with stored or deflated entries.
//!
//! Only the central directory is read when an archive is opened; entry data is read
//! on demand with positioned reads, so one open archive can serve many threads.

use std::fs::File;
use std::io;
use std::path::{Path, PathBuf};

use crate::{Error, normalize};

const EOCD_SIG: u32 = 0x0605_4b50;
const CENTRAL_SIG: u32 = 0x0201_4b50;
const LOCAL_SIG: u32 = 0x0403_4b50;
const EOCD_LEN: usize = 22;
/// The end-of-central-directory record is followed by a comment of at most 64 KiB.
const MAX_EOCD_SEARCH: u64 = EOCD_LEN as u64 + 0xffff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Method {
    Stored,
    Deflated,
}

#[derive(Debug, Clone)]
pub struct Entry {
    /// Normalized path (see [`normalize`]).
    pub name: Box<str>,
    /// Uncompressed size in bytes.
    pub size: u64,
    compressed_size: u64,
    method: Method,
    crc32: u32,
    local_header: u64,
}

#[derive(Debug)]
pub struct Pk3 {
    path: PathBuf,
    file: File,
    entries: Vec<Entry>,
}

impl Pk3 {
    pub fn open(path: impl AsRef<Path>) -> Result<Self, Error> {
        let path = path.as_ref().to_path_buf();
        let file = File::open(&path).map_err(|e| Error::io(&path, e))?;
        let entries = read_central_directory(&file).map_err(|e| e.at(&path))?;
        Ok(Self {
            path,
            file,
            entries,
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// File entries in central-directory order. Directory entries are left out.
    pub fn entries(&self) -> &[Entry] {
        &self.entries
    }

    /// Reads and decompresses an entry, checking its CRC-32.
    pub fn read(&self, entry: &Entry) -> Result<Vec<u8>, Error> {
        self.read_inner(entry).map_err(|e| e.at(&self.path))
    }

    fn read_inner(&self, entry: &Entry) -> Result<Vec<u8>, Error> {
        let mut header = [0u8; 30];
        read_exact_at(&self.file, &mut header, entry.local_header)?;
        let h = Bytes(&header);
        if h.u32(0) != LOCAL_SIG {
            return Err(Error::corrupt(format!("{}: bad local header", entry.name)));
        }
        let data_start = entry.local_header + 30 + u64::from(h.u16(26)) + u64::from(h.u16(28));

        let mut packed = vec![0u8; to_usize(entry.compressed_size)?];
        read_exact_at(&self.file, &mut packed, data_start)?;
        let data = match entry.method {
            Method::Stored => packed,
            Method::Deflated => {
                let mut out = Vec::with_capacity(to_usize(entry.size)?);
                let mut inflater = flate2::Decompress::new(false);
                inflater
                    .decompress_vec(&packed, &mut out, flate2::FlushDecompress::Finish)
                    .map_err(|e| Error::corrupt(format!("{}: {e}", entry.name)))?;
                out
            }
        };
        if data.len() as u64 != entry.size {
            return Err(Error::corrupt(format!(
                "{}: expected {} bytes, got {}",
                entry.name,
                entry.size,
                data.len()
            )));
        }
        if crc32fast::hash(&data) != entry.crc32 {
            return Err(Error::corrupt(format!("{}: CRC mismatch", entry.name)));
        }
        Ok(data)
    }
}

fn read_central_directory(file: &File) -> Result<Vec<Entry>, Error> {
    let len = file.metadata()?.len();
    let tail_len = len.min(MAX_EOCD_SEARCH);
    let mut tail = vec![0u8; to_usize(tail_len)?];
    read_exact_at(file, &mut tail, len - tail_len)?;
    if tail.len() < EOCD_LEN {
        return Err(Error::corrupt("not a zip file (too short)"));
    }
    let eocd = (0..=tail.len().saturating_sub(EOCD_LEN))
        .rev()
        .find(|&i| Bytes(&tail[i..]).u32(0) == EOCD_SIG)
        .ok_or_else(|| Error::corrupt("not a zip file (no end of central directory)"))?;
    let e = Bytes(&tail[eocd..]);
    let count = e.u16(10);
    let dir_size = e.u32(12);
    let dir_offset = e.u32(16);
    if count == 0xffff || dir_size == 0xffff_ffff || dir_offset == 0xffff_ffff {
        return Err(Error::corrupt("ZIP64 archives are not supported"));
    }

    let mut dir = vec![0u8; dir_size as usize];
    read_exact_at(file, &mut dir, u64::from(dir_offset))?;
    let mut entries = Vec::with_capacity(usize::from(count));
    let mut at = 0usize;
    for _ in 0..count {
        let rec = dir
            .get(at..at + 46)
            .ok_or_else(|| Error::corrupt("central directory is truncated"))?;
        let c = Bytes(rec);
        if c.u32(0) != CENTRAL_SIG {
            return Err(Error::corrupt("bad central directory record"));
        }
        let name_len = usize::from(c.u16(28));
        let record_len = 46 + name_len + usize::from(c.u16(30)) + usize::from(c.u16(32));
        let raw_name = dir
            .get(at + 46..at + 46 + name_len)
            .ok_or_else(|| Error::corrupt("central directory is truncated"))?;
        at += record_len;

        let name = normalize(&String::from_utf8_lossy(raw_name));
        if name.is_empty() || name.ends_with('/') {
            continue; // directory entry
        }
        let method = match c.u16(10) {
            0 => Method::Stored,
            8 => Method::Deflated,
            m => {
                return Err(Error::corrupt(format!(
                    "{name}: unsupported compression method {m}"
                )));
            }
        };
        entries.push(Entry {
            name: name.into(),
            crc32: c.u32(16),
            compressed_size: u64::from(c.u32(20)),
            size: u64::from(c.u32(24)),
            method,
            local_header: u64::from(c.u32(42)),
        });
    }
    Ok(entries)
}

/// Little-endian field access into a byte slice whose length the caller has checked.
struct Bytes<'a>(&'a [u8]);

impl Bytes<'_> {
    fn u16(&self, at: usize) -> u16 {
        u16::from_le_bytes([self.0[at], self.0[at + 1]])
    }
    fn u32(&self, at: usize) -> u32 {
        u32::from_le_bytes(self.0[at..at + 4].try_into().unwrap())
    }
}

fn to_usize(n: u64) -> Result<usize, Error> {
    usize::try_from(n).map_err(|_| Error::corrupt("entry too large for this platform"))
}

#[cfg(unix)]
fn read_exact_at(file: &File, buf: &mut [u8], offset: u64) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

#[cfg(windows)]
fn read_exact_at(file: &File, mut buf: &mut [u8], mut offset: u64) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset)? {
            0 => return Err(io::ErrorKind::UnexpectedEof.into()),
            n => {
                buf = &mut buf[n..];
                offset += n as u64;
            }
        }
    }
    Ok(())
}
