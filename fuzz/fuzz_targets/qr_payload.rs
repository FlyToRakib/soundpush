#![no_main]
use libfuzzer_sys::fuzz_target;
use sp_security::pairing::QrPairingPayload;

fuzz_target!(|data: &[u8]| {
    if let Ok(text) = std::str::from_utf8(data) {
        if let Ok(payload) = QrPairingPayload::from_uri(text) {
            // Anything that parses must re-encode to an equivalent code.
            let again = QrPairingPayload::from_uri(&payload.to_uri()).expect("re-encoded code parses");
            assert_eq!(again.device_id, payload.device_id);
            assert_eq!(again.secret, payload.secret);
            assert_eq!(again.addresses, payload.addresses);
        }
    }
});
