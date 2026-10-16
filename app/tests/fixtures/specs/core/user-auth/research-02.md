# Research 02: Refresh token rotation

A refresh token used twice means it was stolen: the whole family is revoked.

```ts
if (token.usedAt !== null) {
  await revokeFamily(token.familyId)
  throw new Unauthorized('refresh token reused')
}
```

1. Issue a new refresh token on each use.
2. Keep the family id across rotations.
3. Revoke the family on reuse.
