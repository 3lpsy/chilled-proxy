use super::{password, session};
use session::{cookie_values, mint_token, set_cookie, token_hash};

#[test]
fn hash_and_verify_round_trip() {
    let phc = password::hash("hunter2").unwrap();
    assert!(phc.starts_with("$argon2"));
    assert!(password::verify("hunter2", &phc));
    assert!(!password::verify("wrong", &phc));
    assert!(!password::verify("hunter2", "not-a-phc-string"));
}

#[test]
fn tokens_are_unique_and_hash_deterministically() {
    let a = mint_token().unwrap();
    let b = mint_token().unwrap();
    assert_ne!(a, b);
    assert_eq!(a.len(), 64);
    assert_eq!(token_hash(&a), token_hash(&a));
    assert_ne!(token_hash(&a), token_hash(&b));
}

#[test]
fn extracts_every_session_cookie() {
    let header = "other=1; chilled_session=aa; theme=dark;chilled_session=bb";
    assert_eq!(cookie_values(header), ["aa", "bb"]);
    assert!(cookie_values("other=x").is_empty());
    assert!(cookie_values("chilled_session=").is_empty());
}

#[test]
fn secure_flag_tracks_https() {
    assert!(set_cookie("t", 60, true).contains("; Secure"));
    assert!(!set_cookie("t", 60, false).contains("; Secure"));
    assert!(session::clear_cookie().contains("Max-Age=0"));
}
