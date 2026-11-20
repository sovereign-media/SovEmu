//! The key ID tokens are signed with: an ES256 key (oauth-as's `EcdsaP256Key`), published at
//! `/oauth/jwks`.
//!
//! The private scalar is derived from a secret string (`SOVEMU_TOKEN_KEY`, else one generated into
//! `server.json` like the asset signing key), so the key survives restarts and can be set from the
//! environment like every other setting.

use oauth_as::jwt::{EcdsaP256Key, Jwks, compact_jws};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub(crate) struct TokenKey {
    key: EcdsaP256Key,
    kid: String,
}

impl std::fmt::Debug for TokenKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TokenKey").field("kid", &self.kid).finish_non_exhaustive()
    }
}

impl TokenKey {
    /// The key for a secret: always the same key for the same secret.
    pub(crate) fn from_secret(secret: &str) -> Self {
        // A hash is a valid P-256 scalar unless it is zero or at least the group order (odds of
        // about 2^-32); a counter moves past one that is not.
        let (key, kid) = (0u32..)
            .find_map(|n| {
                let scalar = Sha256::new()
                    .chain_update(b"sovemu token signing key\0")
                    .chain_update(n.to_be_bytes())
                    .chain_update(secret.as_bytes())
                    .finalize();
                let kid = hex::encode(&Sha256::digest(scalar)[..8]);
                EcdsaP256Key::from_scalar_bytes(kid.clone(), &scalar).ok().map(|k| (k, kid))
            })
            .expect("some counter gives a valid scalar");
        Self { key, kid }
    }

    /// A signed ID token (a compact JWS) carrying `claims`.
    pub(crate) fn sign(&self, claims: &Value) -> String {
        let header = json!({"alg": "ES256", "typ": "JWT", "kid": self.kid});
        compact_jws(header.to_string().as_bytes(), claims.to_string().as_bytes(), |input| {
            self.key.sign_signing_input(input).expect("an in-memory P-256 key signs")
        })
    }

    /// The public key as a JSON Web Key Set.
    pub(crate) fn jwks(&self) -> Value {
        let set = Jwks { keys: vec![self.key.public_jwk()] };
        serde_json::to_value(set).expect("a key set serializes")
    }
}

#[cfg(test)]
mod tests {
    use base64::Engine;
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use oauth_as::jwt::{Jwk, verify_es256};

    use super::*;

    #[test]
    fn tokens_verify_against_the_published_key() {
        let key = TokenKey::from_secret("a secret");
        let token = key.sign(&json!({"sub": "1"}));
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3);
        let header: Value =
            serde_json::from_slice(&URL_SAFE_NO_PAD.decode(parts[0]).unwrap()).unwrap();
        assert_eq!(header["alg"], "ES256");
        let jwks = key.jwks();
        assert_eq!(jwks["keys"][0]["kid"], header["kid"]);
        let public = Jwk::from_json(&jwks["keys"][0]).unwrap();
        let input = format!("{}.{}", parts[0], parts[1]);
        let signature = URL_SAFE_NO_PAD.decode(parts[2]).unwrap();
        assert!(verify_es256(&public, input.as_bytes(), &signature));
        assert!(!verify_es256(&public, b"tampered", &signature));
    }

    #[test]
    fn the_same_secret_gives_the_same_key() {
        let a = TokenKey::from_secret("one");
        assert_eq!(a.jwks(), TokenKey::from_secret("one").jwks());
        assert_ne!(a.jwks(), TokenKey::from_secret("two").jwks());
    }
}
