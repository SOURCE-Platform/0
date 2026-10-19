use crate::{client::EnableBanking, secrets::Credentials};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use rsa::{
    pkcs8::{EncodePrivateKey, LineEnding},
    signature::Verifier,
    RsaPrivateKey,
};
use serde_json::Value;

#[test]
fn jwt_is_rs256_signed_for_enable_banking_with_a_short_lifetime() {
    let key = RsaPrivateKey::new(&mut rsa::rand_core::OsRng, 2048).unwrap();
    let pem = key.to_pkcs8_pem(LineEnding::LF).unwrap();
    let api = EnableBanking::new(&Credentials {
        application_id: "00000000-0000-4000-8000-000000000001".into(),
        private_key: pem.to_string(),
    })
    .unwrap();
    let token = api.jwt().unwrap();
    let parts: Vec<_> = token.split('.').collect();
    assert_eq!(parts.len(), 3);
    let header: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
    let body: Value = serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[1]).unwrap()).unwrap();
    assert_eq!(header["alg"], "RS256");
    assert_eq!(header["kid"], "00000000-0000-4000-8000-000000000001");
    assert_eq!(body["iss"], "enablebanking.com");
    assert_eq!(body["aud"], "api.enablebanking.com");
    assert_eq!(
        body["exp"].as_i64().unwrap() - body["iat"].as_i64().unwrap(),
        300
    );
    let signature =
        rsa::pkcs1v15::Signature::try_from(URL_SAFE_NO_PAD.decode(parts[2]).unwrap().as_slice())
            .unwrap();
    rsa::pkcs1v15::VerifyingKey::<sha2::Sha256>::new(key.to_public_key())
        .verify(format!("{}.{}", parts[0], parts[1]).as_bytes(), &signature)
        .unwrap();
}
