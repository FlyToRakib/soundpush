#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(packet) = sp_protocol::MediaPacket::decode(bytes::Bytes::copy_from_slice(data)) {
        // Receivers accept legacy payloads up to 1200 bytes; senders produce at most
        // MAX_MEDIA_PAYLOAD. Anything a sender may produce must re-encode to the same bytes.
        if packet.payload.len() <= sp_protocol::media::MAX_MEDIA_PAYLOAD {
            let wire = packet.encode().expect("decoded packet re-encodes");
            assert_eq!(&wire[..], data);
        }
    }
});
