#![no_main]
use libfuzzer_sys::fuzz_target;
use sp_protocol::framing::{FrameDecoder, MAX_CONTROL_FRAME};

fuzz_target!(|data: &[u8]| {
    let mut decoder = FrameDecoder::new(MAX_CONTROL_FRAME);
    // Feed in uneven chunks to exercise partial reads.
    for chunk in data.chunks(7) {
        decoder.extend(chunk);
        loop {
            match decoder.next_frame() {
                Ok(Some(frame)) => assert!(frame.len() <= MAX_CONTROL_FRAME),
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
});
