// Core authentication routines: verify a password, then issue a session token.

fn verify_password(username: &str, password: &str) -> bool {
    hash_password(password) == lookup_stored_hash(username)
}

fn hash_password(password: &str) -> String {
    format!("hashed:{}", password)
}

fn lookup_stored_hash(username: &str) -> String {
    format!("hashed:{}-secret", username)
}

fn issue_token(username: &str) -> String {
    format!("token-for-{}", username)
}

fn authenticate_user(username: &str, password: &str) -> Option<String> {
    if verify_password(username, password) {
        Some(issue_token(username))
    } else {
        None
    }
}
