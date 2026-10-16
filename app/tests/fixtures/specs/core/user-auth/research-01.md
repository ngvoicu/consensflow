# Research 01: Password hashing

## Question

Which hash should store the users' passwords?

## Findings

| Hash | Memory-hard | OWASP | Limit |
|------|-------------|-------|-------|
| argon2id | yes | first choice | none |
| scrypt | yes | second | none |
| bcrypt | no | legacy | 72 bytes |

- argon2id with 19 MiB and two passes meets OWASP's minimum.
- bcrypt truncates at 72 bytes, silently.

## Recommendation

argon2id, through the `argon2` crate's bindings. Back to [the spec](SPEC.md).
