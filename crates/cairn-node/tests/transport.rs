use cairn_node::transport;
use std::sync::Arc;

#[tokio::test]
async fn mtls_accepts_pinned_peer_and_rejects_unpinned() {
    let (sk_a, kid_a) = cairn_event::generate_key().unwrap();
    let (sk_b, kid_b) = cairn_event::generate_key().unwrap();
    let (_sk_c, kid_c) = cairn_event::generate_key().unwrap();

    // A trusts B only.
    let kid_b2 = kid_b.clone();
    let trust_a: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_b2);
    // B trusts A only.
    let kid_a2 = kid_a.clone();
    let trust_b: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_a2);

    // A serves; B connects -> handshake succeeds (mutually pinned).
    let server = transport::server_config(&sk_a, trust_a.clone()).unwrap();
    let client_b = transport::client_config(&sk_b, trust_b).unwrap();
    assert!(
        transport::test_handshake(server.clone(), client_b)
            .await
            .is_ok(),
        "mutually-pinned peers must handshake"
    );

    // C (untrusted by A) connects -> A's ClientCertVerifier rejects.
    let trust_c_sees_a: transport::TrustStore = Arc::new(move |pk: &str| pk == kid_a);
    let client_c = transport::client_config(&_sk_c, trust_c_sees_a).unwrap();
    let _ = kid_c;
    let _ = kid_b;
    assert!(
        transport::test_handshake(server, client_c).await.is_err(),
        "an unpinned client must be rejected at the TLS layer"
    );
}
