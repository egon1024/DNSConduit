//! Fixed-capacity DNS wire buffers for transaction slots.

pub const DNS_WIRE_BUFFER_SIZE: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WireBufferError {
    TooLarge { len: usize, max: usize },
}

/// Stack-backed wire buffer for typical UDP DNS messages.
#[derive(Debug)]
pub struct WireBuffer {
    buf: [u8; DNS_WIRE_BUFFER_SIZE],
    len: usize,
}

impl Default for WireBuffer {
    fn default() -> Self {
        Self {
            buf: [0; DNS_WIRE_BUFFER_SIZE],
            len: 0,
        }
    }
}

impl WireBuffer {
    pub fn clear(&mut self) {
        self.len = 0;
    }

    pub fn len(&self) -> usize {
        self.len
    }

    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    pub fn as_slice(&self) -> &[u8] {
        &self.buf[..self.len]
    }

    pub fn set_from_slice(&mut self, data: &[u8]) -> Result<(), WireBufferError> {
        if data.len() > DNS_WIRE_BUFFER_SIZE {
            return Err(WireBufferError::TooLarge {
                len: data.len(),
                max: DNS_WIRE_BUFFER_SIZE,
            });
        }
        self.buf[..data.len()].copy_from_slice(data);
        self.len = data.len();
        Ok(())
    }
}
