// Gecko: dom/base/CharacterDataBuffer.h, dom/base/CharacterDataBuffer.cpp (this build's
// nsTextFragment: the text of a DOM Text node)
//
// Translated for reading only, which is all layout does: the buffer is created from a string
// (`SetTo`) and read through the accessors with Gecko's names. The storage is a `Vec` instead of a
// refcounted StringBuffer or a malloc'd Latin-1 array, but the two forms stay: `Is2b()` is false
// when every character is below 256 (then `Get1b()` has the Latin-1 bytes) and true otherwise
// (`Get2b()`), as in Gecko, because layout code branches on it (nsTextFrame::IsAllWhitespace, the
// 8-bit TransformText).
//
// NOT PORTED: the shared strings for runs of spaces and newlines (sSpaceSharedString), mutation
// (Append, CopyTo, BufferEquals), the SSE2 and VMX scanners, memory reporting, the bidi scan
// (UpdateBidiFlag: the flag is only set by `SetBidi`). The arena DOM (mrt-dom) keeps the text of
// a node as a UTF-8 `&str` (`Dom::text_of`); the frame constructor makes a buffer from that with
// `CharacterDataBuffer::from_str`.
use crate::*;

// Gecko: dom/base/CharacterDataBuffer.h mozilla::dom::CharacterDataBuffer
#[derive(Clone, Debug, Default)]
pub struct CharacterDataBuffer {
    // mState.mIs2b: which of the two is in use
    m1b: Vec<u8>,
    m2b: Vec<u16>,
    mIs2b: bool,
    mIsBidi: bool,
}

impl CharacterDataBuffer {
    // Gecko: CharacterDataBuffer::CharacterDataBuffer
    pub fn new() -> CharacterDataBuffer {
        CharacterDataBuffer::default()
    }

    // ADAPTER: a buffer holding this UTF-8 text (what the arena DOM stores for a text node).
    pub fn from_str(aText: &str) -> CharacterDataBuffer {
        let utf16: Vec<u16> = aText.encode_utf16().collect();
        let mut buffer = CharacterDataBuffer::new();
        buffer.SetTo(&utf16, false, false);
        buffer
    }

    // Gecko: CharacterDataBuffer::Is2b
    pub fn Is2b(&self) -> bool {
        self.mIs2b
    }

    // Gecko: CharacterDataBuffer::IsBidi
    pub fn IsBidi(&self) -> bool {
        self.mIsBidi
    }

    // Gecko: CharacterDataBuffer::Get2b
    pub fn Get2b(&self) -> &[u16] {
        MOZ_ASSERT!(self.Is2b(), "not 2b text");
        &self.m2b
    }

    // Gecko: CharacterDataBuffer::Get1b
    pub fn Get1b(&self) -> &[u8] {
        NS_ASSERTION!(!self.Is2b(), "not 1b text");
        &self.m1b
    }

    // Gecko: CharacterDataBuffer::GetUnsigned1b
    pub fn GetUnsigned1b(&self) -> &[u8] {
        NS_ASSERTION!(!self.Is2b(), "not 1b text");
        &self.m1b
    }

    // Gecko: CharacterDataBuffer::GetLength
    pub fn GetLength(&self) -> u32 {
        if self.mIs2b { self.m2b.len() as u32 } else { self.m1b.len() as u32 }
    }

    // Gecko: dom/base/CharacterDataBuffer.cpp CharacterDataBuffer::SetTo(const char16_t*, uint32_t, bool, bool)
    // Takes the characters as a slice. 1-byte storage is used when every character is below 256 and
    // aForce2b is false.
    pub fn SetTo(&mut self, aBuffer: &[u16], aUpdateBidi: bool, aForce2b: bool) -> bool {
        self.m1b.clear();
        self.m2b.clear();
        self.mIs2b = false;
        self.mIsBidi = false;
        if aBuffer.is_empty() {
            return true;
        }
        // See if we need to store the data in ucs2 or not
        let first16bit = if aForce2b { Some(0) } else { aBuffer.iter().position(|&c| c >= 256) };
        if first16bit.is_some() {
            // Use ucs2 storage because we have to
            self.m2b = aBuffer.to_vec();
            self.mIs2b = true;
            // NOT PORTED: UpdateBidiFlag(aBuffer + first16bit, aLength - first16bit)
            let _ = aUpdateBidi;
        } else {
            // Use 1 byte storage because we can
            self.m1b = aBuffer.iter().map(|&c| c as u8).collect();
        }
        true
    }

    // Gecko: CharacterDataBuffer::AppendTo(nsAString&)
    pub fn AppendTo__nsAString(&self, aString: &mut Vec<u16>) {
        if self.mIs2b {
            aString.extend_from_slice(&self.m2b);
        } else {
            aString.extend(self.m1b.iter().map(|&b| b as u16));
        }
    }

    // Gecko: CharacterDataBuffer::AppendTo(nsAString&, uint32_t, uint32_t)
    pub fn AppendTo__nsAString_uint32_t_uint32_t(&self, aString: &mut Vec<u16>, aOffset: u32, aLength: u32) {
        let (o, l) = (aOffset as usize, aLength as usize);
        if self.mIs2b {
            aString.extend_from_slice(&self.m2b[o..o + l]);
        } else {
            aString.extend(self.m1b[o..o + l].iter().map(|&b| b as u16));
        }
    }

    // Gecko: CharacterDataBuffer::CharAt
    pub fn CharAt(&self, aIndex: u32) -> u16 {
        MOZ_ASSERT!(aIndex < self.GetLength(), "bad index");
        if self.mIs2b { self.m2b[aIndex as usize] } else { self.m1b[aIndex as usize] as u16 }
    }

    // Gecko: CharacterDataBuffer::SafeCharAt
    pub fn SafeCharAt(&self, aIndex: u32) -> u16 {
        if aIndex < self.GetLength() { self.CharAt(aIndex) } else { 0 }
    }

    // Gecko: CharacterDataBuffer::FirstChar
    pub fn FirstChar(&self) -> u16 {
        MOZ_ASSERT!(self.GetLength() != 0);
        self.CharAt(0)
    }

    // Gecko: CharacterDataBuffer::SafeFirstChar
    pub fn SafeFirstChar(&self) -> u16 {
        if self.GetLength() != 0 { self.FirstChar() } else { 0 }
    }

    // Gecko: CharacterDataBuffer::LastChar
    pub fn LastChar(&self) -> u16 {
        MOZ_ASSERT!(self.GetLength() != 0);
        self.CharAt(self.GetLength() - 1)
    }

    // Gecko: CharacterDataBuffer::SafeLastChar
    pub fn SafeLastChar(&self) -> u16 {
        if self.GetLength() != 0 { self.LastChar() } else { 0 }
    }

    // Gecko: CharacterDataBuffer::IsHighSurrogateFollowedByLowSurrogateAt
    pub fn IsHighSurrogateFollowedByLowSurrogateAt(&self, aIndex: u32) -> bool {
        MOZ_ASSERT!(aIndex < self.GetLength());
        if !self.mIs2b || aIndex + 1 >= self.GetLength() {
            return false;
        }
        IsSurrogatePair(self.m2b[aIndex as usize] as u32, self.m2b[aIndex as usize + 1] as u32)
    }

    // Gecko: CharacterDataBuffer::IsLowSurrogateFollowingHighSurrogateAt
    pub fn IsLowSurrogateFollowingHighSurrogateAt(&self, aIndex: u32) -> bool {
        MOZ_ASSERT!(aIndex < self.GetLength());
        if !self.mIs2b || aIndex == 0 {
            return false;
        }
        IsSurrogatePair(self.m2b[aIndex as usize - 1] as u32, self.m2b[aIndex as usize] as u32)
    }

    /**
     * ScalarValueAt() returns a Unicode scalar value at aIndex.  If the character
     * at aIndex is a high surrogate followed by low surrogate, returns character
     * code for the pair.  If the index is low surrogate, or a high surrogate but
     * not in a pair, returns 0.
     */
    // Gecko: CharacterDataBuffer::ScalarValueAt
    pub fn ScalarValueAt(&self, aIndex: u32) -> u32 {
        MOZ_ASSERT!(aIndex < self.GetLength());
        if !self.mIs2b {
            return self.m1b[aIndex as usize] as u32;
        }
        let ch = self.m2b[aIndex as usize];
        if !(0xD800..=0xDFFF).contains(&(ch as u32)) {  // IsSurrogate
            return ch as u32;
        }
        if aIndex + 1 < self.GetLength() && IsHighSurrogate(ch as u32) {
            let nextCh = self.m2b[aIndex as usize + 1];
            if IsLowSurrogate(nextCh as u32) {
                return SurrogateToUCS4(ch, nextCh);
            }
        }
        0
    }

    // Gecko: CharacterDataBuffer::SetBidi
    pub fn SetBidi(&mut self, aBidi: bool) {
        self.mIsBidi = aBidi;
    }

    // Gecko: dom/base/CharacterDataBuffer.cpp CharacterDataBuffer::IsOnlyWhitespace (the test behind
    // nsIContent::TextIsOnlyWhitespace: space, tab, newline, carriage return and form feed)
    pub fn IsOnlyWhitespace(&self) -> bool {
        (0..self.GetLength()).all(|i| matches!(self.CharAt(i), 0x20 | 0x09 | 0x0A | 0x0D | 0x0C))
    }
}
