//! Lossless event framing.
//!
//! An event is one line plus every immediately following line that begins with a space,
//! tab, CR or LF (an indented continuation or a blank line), including all line
//! terminators. Concatenating the yielded ranges and then `remainder()` reproduces the
//! input byte for byte. Framing is format-agnostic by design: it runs before the raw store
//! and therefore before any parser is chosen, so the only continuation rule it may use is
//! one that needs no vendor knowledge.
//!
//! With `eof == false` the framer withholds any event whose end cannot yet be decided
//! (the following line's first byte has not arrived); the caller carries `remainder()` into
//! the next chunk. With `eof == true` everything is consumed.

use std::ops::Range;

pub struct Framer<'a> {
    buf: &'a [u8],
    pos: usize,
    eof: bool,
}

#[inline]
fn is_continuation(first: u8) -> bool {
    matches!(first, b' ' | b'\t' | b'\r' | b'\n')
}

impl<'a> Framer<'a> {
    pub fn new(buf: &'a [u8], eof: bool) -> Self {
        Framer { buf, pos: 0, eof }
    }

    /// Bytes not yet framed. Empty once an `eof` framer is exhausted.
    pub fn remainder(&self) -> &'a [u8] {
        &self.buf[self.pos..]
    }

    /// End of the line starting at `from` (index one past its `\n`), or `None` if the line
    /// is incomplete and more bytes may arrive.
    #[inline]
    fn line_end(&self, from: usize) -> Option<usize> {
        match memchr::memchr(b'\n', &self.buf[from..]) {
            Some(i) => Some(from + i + 1),
            None if self.eof => Some(self.buf.len()),
            None => None,
        }
    }
}

impl Iterator for Framer<'_> {
    type Item = Range<usize>;

    fn next(&mut self) -> Option<Range<usize>> {
        let start = self.pos;
        if start >= self.buf.len() {
            return None;
        }
        let mut end = self.line_end(start)?;
        loop {
            if end >= self.buf.len() {
                if self.eof {
                    break;
                }
                return None; // next line's first byte unknown
            }
            if !is_continuation(self.buf[end]) {
                break;
            }
            end = self.line_end(end)?;
        }
        self.pos = end;
        Some(start..end)
    }
}
