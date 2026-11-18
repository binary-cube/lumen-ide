use super::*;

#[test]
fn sums_are_written_as_pins_are() {
    assert_eq!(hex(&[0x00, 0xab, 0x10]), "00ab10");
    assert_eq!(base64(b"Man"), "TWFu");
    assert_eq!(base64(b"Ma"), "TWE=");
    assert_eq!(base64(b"M"), "TQ==");
    assert_eq!(
        sha256_hex(b"abc"),
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
}
