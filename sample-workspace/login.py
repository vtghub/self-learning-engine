class LoginService:
    def login(self, username, password):
        if self.check_credentials(username, password):
            return self.create_session_token(username)
        return None

    def check_credentials(self, username, password):
        return hash_password(password) == lookup_stored_hash(username)

    def create_session_token(self, username):
        return "session-token-for-" + username


def hash_password(password):
    return "hashed:" + password


def lookup_stored_hash(username):
    return "hashed:" + username + "-secret"
