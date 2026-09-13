#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    if let Ok(packet) = sp_protocol::MediaPacket::decode(bytes::Bytes::copy_from_slice(data)) {
        // Anything that decodes must re-encode to the same bytes.
        let wire = packet.encode().expect("decoded packet re-encodes");
        assert_eq!(&wire[..], data);
    }
});
