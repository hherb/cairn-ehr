// Binding check: the minted cert's SPKI Ed25519 key must equal verifying_key().
#[test]
fn node_cert_spki_equals_verifying_key() {
    let (sk, kid) = cairn_event::generate_key().unwrap();
    let (cert_der, _key) = cairn_node::transport::node_cert(&sk).unwrap();
    let (_, parsed) = x509_parser::parse_x509_certificate(cert_der.as_ref()).unwrap();
    let spki = parsed.public_key().subject_public_key.as_ref();
    assert_eq!(hex::encode(spki), kid, "cert SPKI must pin the signing key");
}
