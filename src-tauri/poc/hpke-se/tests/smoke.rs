#[test]
fn bridge_links_and_se_is_available() {
    let tag = "ov0-poc-smoke";
    let pk = ov0_hpke_poc::se_create(tag).expect("SE key create");
    assert_eq!(pk[0], 0x04, "65-byte uncompressed X9.63 form");
    ov0_hpke_poc::se_delete(tag);
}
