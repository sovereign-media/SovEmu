//! The key ID tokens are signed with: ES256 (ECDSA on P-256 with SHA-256), published at
//! `/oauth/jwks`.
//!
//! The private key is derived from a secret string (`SOVEMU_TOKEN_KEY`, else one generated into
//! `server.json`, as the asset signing key is), so it survives restarts and can be set from the
//! environment like every other setting. Changing the secret changes the key: ID tokens already
//! issued no longer verify, which only matters to a client that checks them later.

use base64::Engine;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use p256::ecdsa::signature::Signer;
use p256::ecdsa::{Signature, SigningKey};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) struct TokenKeys {
    signing: SigningKey,
    /// The key's id in the JWKS and in every token's header: the start of its public key's hash.
    kid: String,
}

impl std::fmt::Debug for TokenKeys {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenKeys").field("kid", &self.kid).finish_non_exhaustive()
    }
}

impl TokenKeys {
    /// The key for a secret: always the same key for the same secret.
    pub(crate) fn from_secret(secret: &str) -> Self {
        // A hash is a valid P-256 scalar unless it is zero or at least the group order, a chance
        // of about 2^-32; a counter moves past one that is not.
        let signing = (0u32..)
            .find_map(|n| {
                let digest = Sha256::new()
                    .chain_update(b"sovemu token signing key\0")
                    .chain_update(n.to_be_bytes())
                    .chain_update(secret.as_bytes())
                    .finalize();
                SigningKey::from_bytes(&digest).ok()
            })
            .expect("some counter gives a valid scalar");
        let point = signing.verifying_key().to_encoded_point(false);
        let kid = hex::encode(&Sha256::digest(point.as_bytes())[..8]);
        Self { signing, kid }
    }

    /// A signed JWT (compact JWS) carrying `claims`.
    pub(crate) fn sign(&self, claims: &Value) -> String {
        let header = json!({"alg": "ES256", "typ": "JWT", "kid": self.kid});
        let input = format!(
            "{}.{}",
            URL_SAFE_NO_PAD.encode(header.to_string()),
            URL_SAFE_NO_PAD.encode(claims.to_string())
        );
        // JWS's ES256 signature is r and s, 32 bytes each: exactly `Signature::to_bytes`.
        let signature: Signature = self.signing.sign(input.as_bytes());
        format!("{input}.{}", URL_SAFE_NO_PAD.encode(signature.to_bytes()))
    }

    /// The public key as a JSON Web Key Set.
    pub(crate) fn jwks(&self) -> Value {
        let point = self.signing.verifying_key().to_encoded_point(false);
        let coordinate = |c: Option<&p256::FieldBytes>| {
            URL_SAFE_NO_PAD.encode(c.expect("an uncompressed point has both coordinates"))
        };
        json!({"keys": [{
            "kty": "EC",
            "crv": "P-256",
            "use": "sig",
            "alg": "ES256",
            "kid": self.kid,
            "x": coordinate(point.x()),
            "y": coordinate(point.y()),
        }]})
    }
}

#[cfg(test)]
mod tests {
    use p256::ecdsa::VerifyingKey;
    use p256::ecdsa::signature::Verifier;

    use super::*;

    fn decode(part: &str) -> Value {
        serde_json::from_slice(&URL_SAFE_NO_PAD.decode(part).unwrap()).unwrap()
    }

    #[test]
    fn tokens_verify_against_the_published_key() {
        let keys = TokenKeys::from_secret("a secret");
        let token = keys.sign(&json!({"sub": "1", "aud": "sovemu-web"}));
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        assert_eq!(decode(parts[0])["alg"], "ES256");
        assert_eq!(decode(parts[1])["sub"], "1");

        // Rebuild the public key from the JWKS alone, as a client would.
        let jwk = &keys.jwks()["keys"][0];
        assert_eq!(jwk["kid"], decode(parts[0])["kid"]);
        let mut sec1 = vec![4u8];
        sec1.extend(URL_SAFE_NO_PAD.decode(jwk["x"].as_str().unwrap()).unwrap());
        sec1.extend(URL_SAFE_NO_PAD.decode(jwk["y"].as_str().unwrap()).unwrap());
        let public = VerifyingKey::from_sec1_bytes(&sec1).unwrap();
        let signature = Signature::from_slice(&URL_SAFE_NO_PAD.decode(parts[2]).unwrap()).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        assert!(public.verify(input.as_bytes(), &signature).is_ok());
        assert!(public.verify(b"tampered", &signature).is_err());
    }

    #[test]
    fn the_same_secret_gives_the_same_key() {
        let a = TokenKeys::from_secret("one");
        assert_eq!(a.jwks(), TokenKeys::from_secret("one").jwks());
        assert_ne!(a.jwks(), TokenKeys::from_secret("two").jwks());
    }
}
