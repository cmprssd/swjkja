//! ICARUS scripts as the game loads them: the compiled `.ibi` block streams
//! (`code/icarus/BlockStream.cpp`), nested as the sequencer routes them
//! (`Sequencer.cpp`: `CSequencer::Route`).
//!
//! A file is "IBI\0", the version as a float (1.57), then blocks: an id, a member
//! count and a flags byte, then each member as an id, a size and that many bytes.
//! Strings keep their terminating NUL; numbers are floats.

/// Token and block ids (`CIcarus`'s enums in `IcarusImplementation.h`).
pub mod id {
    pub const CHAR: i32 = 3;
    pub const STRING: i32 = 4;
    pub const INT: i32 = 5;
    pub const FLOAT: i32 = 6;
    pub const IDENTIFIER: i32 = 7;
    pub const VECTOR: i32 = 14;
    pub const GREATER_THAN: i32 = 15;
    pub const LESS_THAN: i32 = 16;
    pub const EQUALS: i32 = 17;
    pub const NOT: i32 = 18;

    pub const AFFECT: i32 = 19;
    pub const SOUND: i32 = 20;
    pub const MOVE: i32 = 21;
    pub const ROTATE: i32 = 22;
    pub const WAIT: i32 = 23;
    pub const BLOCK_START: i32 = 24;
    pub const BLOCK_END: i32 = 25;
    pub const SET: i32 = 26;
    pub const LOOP: i32 = 27;
    pub const LOOPEND: i32 = 28;
    pub const PRINT: i32 = 29;
    pub const USE: i32 = 30;
    pub const FLUSH: i32 = 31;
    pub const RUN: i32 = 32;
    pub const KILL: i32 = 33;
    pub const REMOVE: i32 = 34;
    pub const CAMERA: i32 = 35;
    pub const GET: i32 = 36;
    pub const RANDOM: i32 = 37;
    pub const IF: i32 = 38;
    pub const ELSE: i32 = 39;
    pub const REM: i32 = 40;
    pub const TASK: i32 = 41;
    pub const DO: i32 = 42;
    pub const DECLARE: i32 = 43;
    pub const FREE: i32 = 44;
    pub const DOWAIT: i32 = 45;
    pub const SIGNAL: i32 = 46;
    pub const WAITSIGNAL: i32 = 47;
    pub const PLAY: i32 = 48;
    pub const TAG: i32 = 49;
}

/// The version every `.ibi` is written with (`s_IBI_VERSION`).
pub const VERSION: f32 = 1.57;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

/// One value of a block: a token id ([`id`]) and its bytes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    pub id: i32,
    pub data: Vec<u8>,
}

impl Member {
    /// The text of a string, identifier or character member, up to its NUL.
    pub fn text(&self) -> Option<&str> {
        if !matches!(self.id, id::STRING | id::IDENTIFIER | id::CHAR) {
            return None;
        }
        let end = self
            .data
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(self.data.len());
        std::str::from_utf8(&self.data[..end]).ok()
    }

    /// The value of a float member.
    pub fn float(&self) -> Option<f32> {
        let bytes: [u8; 4] = self.data.as_slice().try_into().ok()?;
        (self.id == id::FLOAT).then(|| f32::from_le_bytes(bytes))
    }
}

/// One command or marker of the stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: i32,
    pub flags: u8,
    pub members: Vec<Member>,
}

/// Reads an `.ibi` file's blocks (`CBlockStream::Open`, `ReadBlock`, `ReadMember`).
/// A `random` member's bytes are read as any other's; the game sets them aside.
pub fn read(bytes: &[u8]) -> Result<Vec<Block>, Error> {
    struct Cursor<'a>(&'a [u8], usize);
    impl<'a> Cursor<'a> {
        fn take(&mut self, n: usize) -> Result<&'a [u8], Error> {
            let at = self.1;
            let s = (self.0)
                .get(at..at + n)
                .ok_or_else(|| Error(format!("truncated at byte {at}")))?;
            self.1 += n;
            Ok(s)
        }
        fn int(&mut self) -> Result<i32, Error> {
            Ok(i32::from_le_bytes(self.take(4)?.try_into().unwrap()))
        }
    }
    if bytes.get(..4) != Some(b"IBI\0") {
        return Err(Error("not an IBI file".into()));
    }
    let mut c = Cursor(bytes, 4);
    let version = f32::from_bits(c.int()? as u32);
    if version != VERSION {
        return Err(Error(format!("version {version}, not {VERSION}")));
    }
    let mut blocks = Vec::new();
    while c.1 < bytes.len() {
        let id = c.int()?;
        let count = c.int()?;
        let flags = c.take(1)?[0];
        if count < 0 {
            return Err(Error(format!("block {id} has {count} members")));
        }
        let mut members = Vec::new();
        for _ in 0..count {
            let id = c.int()?;
            let size = c.int()?;
            let size = usize::try_from(size).map_err(|_| Error(format!("member size {size}")))?;
            members.push(Member {
                id,
                data: c.take(size)?.to_vec(),
            });
        }
        blocks.push(Block { id, flags, members });
    }
    Ok(blocks)
}

/// A block as the sequencer routes it: a command, or a block that opens a body of
/// its own (`affect`, `loop`, `if`, `else`, `task`) up to its `BLOCK_END`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Command(Block),
    Body(Block, Vec<Node>),
}

/// Whether blocks of `id` open a body.
fn opens_body(id: i32) -> bool {
    matches!(id, id::AFFECT | id::LOOP | id::IF | id::ELSE | id::TASK)
}

/// Whether the sequencer takes blocks of `id` as commands (`Route`'s cases).
fn is_command(id: i32) -> bool {
    matches!(
        id,
        id::WAIT
            | id::PRINT
            | id::SOUND
            | id::MOVE
            | id::ROTATE
            | id::SET
            | id::USE
            | id::REMOVE
            | id::KILL
            | id::FLUSH
            | id::CAMERA
            | id::DO
            | id::DECLARE
            | id::FREE
            | id::SIGNAL
            | id::WAITSIGNAL
            | id::PLAY
            | id::RUN
    )
}

/// Nests `blocks` as `Route` does. A body ends at its `BLOCK_END`; an `else` must
/// follow an `if`'s body; a `BLOCK_END` at the top ends the script, as it returns
/// from `Route` there; anything else the sequencer does not route is an error.
pub fn nest(blocks: Vec<Block>) -> Result<Vec<Node>, Error> {
    fn body(it: &mut std::vec::IntoIter<Block>, top: bool) -> Result<Vec<Node>, Error> {
        let mut nodes: Vec<Node> = Vec::new();
        while let Some(b) = it.next() {
            match b.id {
                id::BLOCK_END => return Ok(nodes),
                id::ELSE if !matches!(nodes.last(), Some(Node::Body(b, _)) if b.id == id::IF) => {
                    return Err(Error("an else without an if before it".into()));
                }
                i if opens_body(i) => {
                    let inner = body(it, false)?;
                    nodes.push(Node::Body(b, inner));
                }
                i if is_command(i) => nodes.push(Node::Command(b)),
                i => return Err(Error(format!("block id {i} is not routed"))),
            }
        }
        if top {
            Ok(nodes)
        } else {
            Err(Error("a body has no end".into()))
        }
    }
    body(&mut blocks.into_iter(), true)
}

/// Reads and nests an `.ibi` file.
pub fn parse(bytes: &[u8]) -> Result<Vec<Node>, Error> {
    nest(read(bytes)?)
}
