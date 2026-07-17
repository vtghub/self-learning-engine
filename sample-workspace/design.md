# Authentication Design

This document describes how the system authenticates a user.

## Password Verification

When a user submits a username and password, the system hashes the password
and compares it against the stored hash for that username. A mismatch
means the credentials are rejected.

## Session Tokens

Once the password is verified, the system issues a session token for the
user. The token is used to authenticate later requests without asking for
the password again.

## Login Flow

The login flow ties the two steps together: verify the password, then
create and return a session token for the authenticated user.
