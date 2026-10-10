//! Jedi Academy models: MD3 meshes ([`md3`]) and Ghoul2 meshes ([`glm`]) with the
//! skeletons they are skinned by ([`gla`]).

pub mod gla;
pub mod glm;
pub mod md3;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

macro_rules! bail {
    ($($t:tt)*) => { return Err($crate::Error(format!($($t)*))) };
}
pub(crate) use bail;

/// Bounds-checked little-endian reads.
pub(crate) struct Reader<'a> {
    pub data: &'a [u8],
    pub what: &'a str,
}

impl Reader<'_> {
    pub fn bytes(&self, at: usize, n: usize) -> Result<&[u8], Error> {
        match at.checked_add(n).and_then(|end| self.data.get(at..end)) {
            Some(b) => Ok(b),
            None => bail!(
                "{}: {n} bytes at {at} run past the end ({} bytes)",
                self.what,
                self.data.len()
            ),
        }
    }
    pub fn i32(&self, at: usize) -> Result<i32, Error> {
        Ok(i32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    pub fn u32(&self, at: usize) -> Result<u32, Error> {
        Ok(u32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    pub fn f32(&self, at: usize) -> Result<f32, Error> {
        Ok(f32::from_le_bytes(self.bytes(at, 4)?.try_into().unwrap()))
    }
    pub fn i16(&self, at: usize) -> Result<i16, Error> {
        Ok(i16::from_le_bytes(self.bytes(at, 2)?.try_into().unwrap()))
    }
    pub fn vec3(&self, at: usize) -> Result<[f32; 3], Error> {
        Ok([self.f32(at)?, self.f32(at + 4)?, self.f32(at + 8)?])
    }
    /// A NUL-terminated string in a fixed field.
    pub fn name(&self, at: usize, n: usize) -> Result<String, Error> {
        let b = self.bytes(at, n)?;
        let end = b.iter().position(|&c| c == 0).unwrap_or(n);
        Ok(b[..end].iter().map(|&c| c as char).collect())
    }
    /// A count field: not negative.
    pub fn count(&self, at: usize, field: &str) -> Result<usize, Error> {
        let n = self.i32(at)?;
        usize::try_from(n).map_err(|_| Error(format!("{}: {field} is {n}", self.what)))
    }
}
