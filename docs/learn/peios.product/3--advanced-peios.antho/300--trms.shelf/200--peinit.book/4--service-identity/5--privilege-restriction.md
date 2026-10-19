---
title: Privilege Restriction
description: peinit only ever removes privileges from a token and never adds one, on either identity path.
---

`RequiredPrivileges` is a list of the privileges a service needs.
Everything else is removed from its token before exec.
[*priv.everything-outside-requiredprivileges-is-removed]

## Subtractive only [*priv.peinit-never-adds-a-privilege]

peinit removes privileges. It never adds one, on either path — there is
no code that constructs anything but a removal. A service cannot acquire
a privilege by naming it in `RequiredPrivileges`; if the token it was
given does not have it, listing it changes nothing.

The removal targets the token's **present** bitmask, through
`KACS_IOC_ADJUST_PRIVS` on the token descriptor — one `kacs_priv_entry`
per removed privilege, carrying the privilege-removed attribute. The
right required on the descriptor is `KACS_TOKEN_ADJUST_PRIVS`, which a
freshly minted token always has.

peinit does not use `KACS_IOC_RESTRICT`. That builds a restricted-SID
token — a different mechanism with different semantics — and does not
touch the privilege bitmask at all. It is available in the bindings and
would be a plausible-looking mistake.

Removing a privilege clears its present, enabled and enabled-by-default
bits together, and irreversibly.
[*priv.a-removal-is-total-and-irreversible] The privileges that
*survive* keep the enable state their source gave them: peinit does not
enable, disable, or re-order anything.
[*priv.survivors-keep-their-source-enable-state] Enable policy belongs
to whoever minted the token.

peinit iterates all sixty-four privilege bits rather than only the ones
it has names for, so a privilege this build does not know about is
stripped along with the rest.
[*priv.privileges-this-build-cannot-name-are-stripped-too] The safe
direction is to remove what was not asked for, including what cannot be
named.

If `RequiredPrivileges` is absent, peinit does not query or adjust the
token at all, and the source's default privilege set stands unchanged.
[*priv.an-absent-list-leaves-the-token-untouched]

## Names [*priv.an-unmatched-privilege-name-fails-the-start]

Privilege names are matched case-sensitively against the published
privilege table. A name that does not match exactly is not silently
ignored: it fails token materialisation, and therefore fails the service
start.

Two privileges KACS enforces are absent from the published table —
`SeTakeOwnershipPrivilege` and `SeRelabelPrivilege` — and so cannot be
named in `RequiredPrivileges` at all. A service that needs either
declares nothing and takes the source token's defaults, or fails to
start if it tries to name one.
[*priv.takeownership-and-relabel-cannot-be-named]
