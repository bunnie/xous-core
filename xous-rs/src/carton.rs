//! A Carton is an object that wraps another object for shipping across the kernel
//! boundary. Structs that are stored in Cartons can be sent as messages.

use crate::{Error, MemoryMessage, MemoryRange, MemorySize, Message, CID};

#[derive(Debug)]
pub struct Carton<'a> {
    range: MemoryRange,
    valid: MemoryRange,
    slice: &'a [u8],
}

impl<'a> Carton<'a> {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        let src_mem = bytes.as_ptr();

        // Ensure our byte size is a multiple of 4096
        let remainder = bytes.len() & 4095;
        let size = bytes.len() + (4096 - remainder);

        let new_mem = crate::map_memory(
            None,
            None,
            size,
            crate::MemoryFlags::R | crate::MemoryFlags::W,
        )
        .unwrap();

        // NOTE: Remaining bytes are not zeroed. We assume the kernel has done this for us.
        unsafe {
            core::ptr::copy(src_mem, new_mem.as_mut_ptr(), bytes.len());
        };
        let mut valid = new_mem;
        valid.size = MemorySize::new(bytes.len()).unwrap();
        Carton {
            range: new_mem,
            slice: unsafe { core::slice::from_raw_parts_mut(new_mem.as_mut_ptr(), bytes.len()) },
            valid,
        }
    }

    pub fn into_message(self, id: usize) -> MemoryMessage {
        MemoryMessage {
            id,
            buf: self.valid,
            offset: None,
            valid: None,
        }
    }

    /// Perform an immutable lend of this Carton to the specified server.
    /// This function will block until the server returns.
    pub fn lend(&self, connection: CID, id: usize) -> Result<(), Error> {
        let msg = MemoryMessage {
            id,
            buf: self.valid,
            offset: None,
            valid: None,
        };
        crate::try_send_message(connection, Message::Borrow(msg))
    }

    /// Perform a mutable lend of this Carton to the server.
    pub fn lend_mut(&mut self, connection: CID, id: usize) -> Result<(), Error> {
        let msg = MemoryMessage {
            id,
            buf: self.valid,
            offset: None,
            valid: None,
        };
        crate::try_send_message(connection, Message::MutableBorrow(msg))
    }
}

impl<'a> AsRef<MemoryRange> for Carton<'a> {
    fn as_ref(&self) -> &MemoryRange {
        &self.valid
    }
}

impl<'a> AsRef<[u8]> for Carton<'a> {
    fn as_ref(&self) -> &[u8] {
        &self.slice
    }
}

impl<'a> Drop for Carton<'a> {
    fn drop(&mut self) {
        crate::unmap_memory(self.range).unwrap();
    }
}
