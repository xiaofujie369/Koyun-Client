use client_platform::{crypto::*, scope::TenantId};

fn key() -> MasterKey {
    MasterKey::from_hex(&"42".repeat(32)).unwrap()
}

#[test]
fn encrypted_credentials_are_bound_to_tenant_account_and_purpose() {
    let key = key();
    let context = b"panel_credential:alpha:account-1";
    let encrypted = key.encrypt(b"private panel token", context).unwrap();
    assert_eq!(
        &*key.decrypt(&encrypted, context).unwrap(),
        b"private panel token"
    );
    for wrong in [
        b"panel_credential:beta:account-1".as_slice(),
        b"panel_credential:alpha:account-2",
        b"webhook:alpha:account-1",
    ] {
        assert_eq!(
            key.decrypt(&encrypted, wrong),
            Err(CryptoError::InvalidCiphertext)
        );
    }
}

#[test]
fn tampered_truncated_wrong_version_and_wrong_key_fail_closed() {
    let context = b"test-context";
    let encrypted = key().encrypt(b"secret", context).unwrap();
    for index in [0, 1, encrypted.len() - 1] {
        let mut changed = encrypted.clone();
        changed[index] ^= 1;
        assert_eq!(
            key().decrypt(&changed, context),
            Err(CryptoError::InvalidCiphertext)
        );
    }
    assert_eq!(
        key().decrypt(&encrypted[..15], context),
        Err(CryptoError::InvalidCiphertext)
    );
    let other = MasterKey::from_hex(&"24".repeat(32)).unwrap();
    assert_eq!(
        other.decrypt(&encrypted, context),
        Err(CryptoError::InvalidCiphertext)
    );
}

#[test]
fn encryption_uses_a_fresh_nonce() {
    let key = key();
    assert_ne!(
        key.encrypt(b"same", b"same").unwrap(),
        key.encrypt(b"same", b"same").unwrap()
    );
}

#[test]
fn tokens_have_distinct_purposes_and_tenant_scoped_hashes() {
    let tenant = TenantId::parse("alpha").unwrap();
    let token = generate_token(&tenant, TokenKind::Refresh).unwrap();
    let (parsed, hash) = inspect_token(token.plaintext.expose(), TokenKind::Refresh).unwrap();
    assert_eq!(parsed, tenant);
    assert_eq!(hash, token.hash);
    assert_eq!(
        inspect_token(token.plaintext.expose(), TokenKind::Access),
        Err(CryptoError::InvalidToken)
    );
    assert_ne!(
        token.hash,
        generate_token(&tenant, TokenKind::Refresh).unwrap().hash
    );
    assert!(!format!("{:?}", token.plaintext).contains(token.plaintext.expose()));
    let changed = token.plaintext.expose().replace("alpha", "beta");
    assert_ne!(
        inspect_token(&changed, TokenKind::Refresh).unwrap().1,
        token.hash
    );
}

#[test]
fn malformed_tokens_and_keys_are_rejected() {
    for token in [
        "",
        "a1.alpha.short",
        "a1...",
        "a1.unsafe/tenant.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "a1.alpha.aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa.extra",
    ] {
        assert_eq!(
            inspect_token(token, TokenKind::Access),
            Err(CryptoError::InvalidToken)
        );
    }
    assert!(MasterKey::from_hex("short").is_err());
    assert!(MasterKey::from_hex(&"zz".repeat(32)).is_err());
}
