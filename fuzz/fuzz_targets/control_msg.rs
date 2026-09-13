#![no_main]
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = sp_protocol::control::ControlMsg::from_bytes(data);
});
