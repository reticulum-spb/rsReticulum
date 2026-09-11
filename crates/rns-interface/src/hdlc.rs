//! HDLC-like framing shared by TCP, Serial, I2P, Local, Pipe, Backbone.
//! Wire: `[FLAG][escaped_payload][FLAG]`. Encode ESC before FLAG; decode FLAG before ESC.

/// HDLC frame delimiter.
pub const FLAG: u8 = 0x7E;

/// HDLC escape byte.
pub const ESC: u8 = 0x7D;

/// XOR mask applied to escaped bytes.
pub const ESC_MASK: u8 = 0x20;

/// Upper bound on a single HDLC frame (sized for full resource transfers on TCP/Local MTUs).
const MAX_FRAME_SIZE: usize = 524288;

/// Pre-allocated capacity for the deframer accumulator. Covers typical HW_MTUs
/// with headroom; larger frames grow the Vec normally.
const DEFRAME_BUF_CAPACITY: usize = 2048;

/// Append the escape-encoded form of `data` to `out`. No allocation if `out` has capacity.
pub fn escape_into(data: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < data.len() {
        match data[i..].iter().position(|&b| b == ESC || b == FLAG) {
            Some(rel) => {
                let hit = i + rel;
                out.extend_from_slice(&data[i..hit]);
                out.push(ESC);
                out.push(data[hit] ^ ESC_MASK);
                i = hit + 1;
            }
            None => {
                out.extend_from_slice(&data[i..]);
                return;
            }
        }
    }
}

/// Append the unescaped form of `data` to `out`. Unknown escape sequences and a
/// trailing lone ESC byte are passed through unchanged — matching the previous
/// implementation so any consumer relying on length/CRC rejection still sees
/// the same bytes.
pub fn unescape_into(data: &[u8], out: &mut Vec<u8>) {
    let mut i = 0;
    while i < data.len() {
        match data[i..].iter().position(|&b| b == ESC) {
            Some(rel) => {
                let hit = i + rel;
                out.extend_from_slice(&data[i..hit]);
                if hit + 1 < data.len() {
                    let next = data[hit + 1];
                    if next == (FLAG ^ ESC_MASK) {
                        out.push(FLAG);
                    } else if next == (ESC ^ ESC_MASK) {
                        out.push(ESC);
                    } else {
                        out.push(ESC);
                        out.push(next);
                    }
                    i = hit + 2;
                } else {
                    out.push(ESC);
                    return;
                }
            }
            None => {
                out.extend_from_slice(&data[i..]);
                return;
            }
        }
    }
}

/// Append a complete HDLC frame for `data` to `out`.
pub fn frame_into(data: &[u8], out: &mut Vec<u8>) {
    out.push(FLAG);
    escape_into(data, out);
    out.push(FLAG);
}

pub fn escape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 8);
    escape_into(data, &mut out);
    out
}

pub fn unescape(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len());
    unescape_into(data, &mut out);
    out
}

pub fn frame(data: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(data.len() + data.len() / 8 + 2);
    frame_into(data, &mut out);
    out
}

pub struct HdlcDeframer {
    max_encoded_size: usize,
    max_decoded_size: Option<usize>,
    oversized_frames: u64,
    buffer: Vec<u8>,
    in_frame: bool,
    /// Reusable scratch for unescape output to avoid per-frame allocation.
    unescape_scratch: Vec<u8>,
}

impl HdlcDeframer {
    pub fn new() -> Self {
        Self {
            max_encoded_size: MAX_FRAME_SIZE,
            max_decoded_size: None,
            oversized_frames: 0,
            buffer: Vec::with_capacity(DEFRAME_BUF_CAPACITY),
            in_frame: false,
            unescape_scratch: Vec::with_capacity(DEFRAME_BUF_CAPACITY),
        }
    }

    /// Bound decoded payload independently from its worst-case HDLC expansion.
    /// Delimiters are not stored in the encoded accumulator. Other drivers
    /// keep the legacy encoded-size limit through `new()`.
    pub fn with_max_decoded_size(limit: usize) -> Self {
        Self {
            max_encoded_size: limit.saturating_mul(2),
            max_decoded_size: Some(limit),
            ..Self::new()
        }
    }

    pub fn oversized_frames(&self) -> u64 {
        self.oversized_frames
    }

    pub fn feed(&mut self, data: &[u8]) -> Vec<Vec<u8>> {
        let mut frames = Vec::new();
        let mut i = 0;
        while i < data.len() {
            match data[i..].iter().position(|&b| b == FLAG) {
                Some(rel) => {
                    let hit = i + rel;
                    if self.in_frame {
                        self.append_in_frame(&data[i..hit]);
                        if !self.buffer.is_empty() {
                            self.unescape_scratch.clear();
                            unescape_into(&self.buffer, &mut self.unescape_scratch);
                            if self
                                .max_decoded_size
                                .is_some_and(|limit| self.unescape_scratch.len() > limit)
                            {
                                self.oversized_frames = self.oversized_frames.saturating_add(1);
                            } else if !self.unescape_scratch.is_empty() {
                                frames.push(std::mem::take(&mut self.unescape_scratch));
                                // Restore capacity for the next frame.
                                self.unescape_scratch = Vec::with_capacity(DEFRAME_BUF_CAPACITY);
                            }
                        }
                        self.buffer.clear();
                    }
                    self.in_frame = true;
                    i = hit + 1;
                }
                None => {
                    if self.in_frame {
                        self.append_in_frame(&data[i..]);
                    }
                    return frames;
                }
            }
        }
        frames
    }

    /// Append `chunk` to `self.buffer` while enforcing the encoded limit. On
    /// overflow the in-progress frame is dropped and we wait for the next FLAG
    /// to resync — matching the previous byte-loop behavior.
    fn append_in_frame(&mut self, chunk: &[u8]) {
        if chunk.len() > self.max_encoded_size.saturating_sub(self.buffer.len()) {
            self.oversized_frames = self.oversized_frames.saturating_add(1);
            self.buffer.clear();
            self.in_frame = false;
            return;
        }
        self.buffer.extend_from_slice(chunk);
    }

    pub fn reset(&mut self) {
        self.buffer.clear();
        self.in_frame = false;
    }
}

impl Default for HdlcDeframer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires local Python 1.5.2 source; RNS_PYTHON_ROOT/RNS_PYTHON_BIN override defaults"]
    fn decoded_limits_match_python_complete_frames() {
        use std::io::Write;
        use std::process::{Command, Stdio};
        let mut input = String::new();
        let mut expected = String::new();
        for limit in [500, 564, 32784, 524352] {
            for size in [limit - 1, limit, limit + 1] {
                for byte in [42, FLAG, ESC] {
                    let mut decoder = HdlcDeframer::with_max_decoded_size(limit);
                    let payload = vec![byte; size];
                    let frames = decoder.feed(&frame(&payload));
                    if size <= limit {
                        assert_eq!(frames, [payload]);
                    }
                    input.push_str(&format!("{limit} {size} {byte}\n"));
                    expected.push_str(&format!(
                        "{} {}\n",
                        frames.len(),
                        decoder.oversized_frames()
                    ));
                }
            }
        }
        let mut child = Command::new(
            std::env::var("RNS_PYTHON_BIN").unwrap_or_else(|_| "/usr/bin/python3.11".into()),
        )
        .arg("-B")
        .arg("-c")
        .arg(
            r#"
import sys
from pathlib import Path
scope = {}
source = Path(sys.argv[1]) / 'RNS/Interfaces/util/HDLC.py'
exec(compile(source.read_text(), str(source), 'exec'), scope)
for line in sys.stdin.readlines():
    limit, size, byte = map(int, line.split())
    frames, invalid = [], []
    receiver = scope['ReceiveBuffer'](mtu=limit, min_frame_len=19, max_frame_len=limit,
        on_frame=frames.append, on_invalid=invalid.append)
    receiver.feed(b'\x7e' + scope['HDLC'].escape(bytes([byte])*size) + b'\x7e')
    print(len(frames), len(invalid))
"#,
        )
        .arg(std::env::var("RNS_PYTHON_ROOT").unwrap_or_else(|_| "/home/room/src/Reticulum".into()))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        let output = child.wait_with_output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_eq!(String::from_utf8(output.stdout).unwrap(), expected);
    }

    #[test]
    fn decoded_limit_accepts_full_escaped_mtu_with_ifac_across_chunks() {
        let limit = 524_288 + 64;
        let payload = vec![FLAG; limit];
        let wire = frame(&payload);
        assert!(wire.len() > MAX_FRAME_SIZE);
        for chunk_size in [1_023, 65_536, wire.len()] {
            let mut decoder = HdlcDeframer::with_max_decoded_size(limit);
            let mut frames = Vec::new();
            for chunk in wire.chunks(chunk_size) {
                frames.extend(decoder.feed(chunk));
                assert!(decoder.buffer.len() <= 2 * limit);
            }
            assert_eq!(frames, [payload.clone()]);
            assert_eq!(decoder.oversized_frames(), 0);
        }
    }

    #[test]
    fn decoded_and_encoded_overflow_drop_once_then_resynchronise() {
        for payload in [vec![42; 101], vec![ESC; 101], vec![42; 1000]] {
            let mut decoder = HdlcDeframer::with_max_decoded_size(100);
            let mut frames = Vec::new();
            for chunk in frame(&payload).chunks(7) {
                frames.extend(decoder.feed(chunk));
                assert!(decoder.buffer.len() <= 200);
            }
            assert!(frames.is_empty());
            assert_eq!(decoder.oversized_frames(), 1);
            assert_eq!(decoder.feed(&frame(&[FLAG; 100])), [vec![FLAG; 100]]);
            assert_eq!(decoder.oversized_frames(), 1);
            decoder.feed(&[FLAG, ESC]);
            decoder.reset();
            assert_eq!(
                decoder.feed(&frame(b"after reset")),
                [b"after reset".to_vec()]
            );
        }
    }

    #[test]
    fn test_escape_unescape_roundtrip() {
        let data = vec![0x00, FLAG, ESC, 0xFF, FLAG, ESC, 0x42];
        let escaped = escape(&data);
        let unescaped = unescape(&escaped);
        assert_eq!(unescaped, data);
    }

    #[test]
    fn test_escape_no_special_bytes() {
        let data = b"hello world";
        let escaped = escape(data);
        assert_eq!(escaped, data);
    }

    #[test]
    fn test_escape_flag_byte() {
        let data = vec![FLAG];
        let escaped = escape(&data);
        assert_eq!(escaped, vec![ESC, FLAG ^ ESC_MASK]);
    }

    #[test]
    fn test_escape_esc_byte() {
        let data = vec![ESC];
        let escaped = escape(&data);
        assert_eq!(escaped, vec![ESC, ESC ^ ESC_MASK]);
    }

    #[test]
    fn test_frame_and_deframe() {
        let data = b"test packet data";
        let framed = frame(data);

        let mut deframer = HdlcDeframer::new();
        let frames = deframer.feed(&framed);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], data);
    }

    #[test]
    fn test_deframer_multiple_frames() {
        let f1 = frame(b"first");
        let f2 = frame(b"second");

        let mut combined = f1;
        combined.extend_from_slice(&f2);

        let mut deframer = HdlcDeframer::new();
        let frames = deframer.feed(&combined);
        assert_eq!(frames.len(), 2);
        assert_eq!(frames[0], b"first");
        assert_eq!(frames[1], b"second");
    }

    #[test]
    fn test_deframer_streaming() {
        let framed = frame(b"streamed");

        let mut deframer = HdlcDeframer::new();

        let mid = framed.len() / 2;
        let frames1 = deframer.feed(&framed[..mid]);
        assert!(frames1.is_empty());

        let frames2 = deframer.feed(&framed[mid..]);
        assert_eq!(frames2.len(), 1);
        assert_eq!(frames2[0], b"streamed");
    }

    #[test]
    fn test_tricky_sequence() {
        // ESC, FLAG^MASK must decode to a single FLAG byte.
        let data = vec![ESC, 0x5E];
        let unescaped = unescape(&data);
        assert_eq!(unescaped, vec![FLAG]);
    }

    #[test]
    fn test_into_variants_match_allocating() {
        // The allocating wrappers must produce byte-identical output.
        let inputs: &[&[u8]] = &[
            b"",
            b"plain",
            &[FLAG, ESC, 0x00, 0xFF, FLAG],
            &[ESC, ESC, ESC],
            &[0x00; 1024],
        ];
        for data in inputs {
            let mut buf = Vec::new();
            escape_into(data, &mut buf);
            assert_eq!(buf, escape(data));

            let escaped = escape(data);
            let mut buf2 = Vec::new();
            unescape_into(&escaped, &mut buf2);
            assert_eq!(buf2, unescape(&escaped));

            let mut buf3 = Vec::new();
            frame_into(data, &mut buf3);
            assert_eq!(buf3, frame(data));
        }
    }

    #[test]
    fn test_deframer_oversized_drops_then_resyncs() {
        // A frame body larger than MAX_FRAME_SIZE must drop and resync on the
        // next FLAG, exactly like the per-byte implementation.
        let mut deframer = HdlcDeframer::new();
        let mut oversized = vec![FLAG];
        oversized.extend(std::iter::repeat_n(0x55, MAX_FRAME_SIZE + 100));
        oversized.push(FLAG);
        let frames = deframer.feed(&oversized);
        assert!(frames.is_empty());

        let small = frame(b"after-overflow");
        let frames = deframer.feed(&small);
        assert_eq!(frames.len(), 1);
        assert_eq!(frames[0], b"after-overflow");
    }
}
