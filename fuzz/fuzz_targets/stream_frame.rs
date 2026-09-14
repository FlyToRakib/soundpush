#![no_main]
use libfuzzer_sys::fuzz_target;
use sp_protocol::framing::StreamFrameDecoder;

// TLS-over-TCP typed frames (USB via adb reverse), plus the probe datagrams they may carry.
fuzz_target!(|data: &[u8]| {
    let mut decoder = StreamFrameDecoder::new();
    for chunk in data.chunks(11) {
        decoder.extend(chunk);
        loop {
            match decoder.next_frame() {
                Ok(Some((kind, frame))) => {
                    assert!(frame.len() <= kind.max_len());
                    let _ = sp_protocol::probe::Probe::decode(&frame);
                }
                Ok(None) => break,
                Err(_) => return,
            }
        }
    }
});
