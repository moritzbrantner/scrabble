# Routing parameter cardinality

Authority: Decision Atlas **6.5**, owner option A on #234 and issue #237.

At the browser URL interpretation boundary, every recognized query key is optional
and single-valued: `match`, `view`, `lang`, `server`, `api`, `certificate`, `share`,
`launch`, `session` and `fixture`. Two or more occurrences of any recognized key
reject the entire route before lookup, creation, admission, account-library access
or transport connection. This includes identical values, empty values, and keys
that become identical after normal URL decoding. There are no duplicate exceptions;
query order never selects a winner. Development-only keys follow the same
cardinality rule in release builds, even though their values cannot override the
compiled production connection settings.

The error explains that the link repeats a routing parameter and asks for a
corrected link with each parameter appearing once. It never displays parameter
values or chooses a substitute game, service, share or interface language. The
existing saved/browser language fallback may localize the error when `lang` is
ambiguous; it does not make the route valid. Preferences remain available, but
changing them does not resume an initially rejected route; open a corrected link
or reload after correcting the address.

A recognized key occurring once keeps its existing validation and meaning.
Unrecognized query keys, including repeated unknown keys, do not trigger this
error and remain in the current URL. Rejection does not rewrite the address or
forward its contents elsewhere. Generated invitations keep their existing public
field whitelist; this policy does not broaden what a new invitation copies.

## Invalid links and recovery

Authority: Decision Atlas **6.5**, issue #238 (owner option A on #234).

`view` is case-sensitive. Its recognized values are `player`, `board`, `layout`
and `word-list`; any other supplied value, including an empty value, shows an
explicit unrecognized-link error before match lookup, creation or admission.
Malformed board match IDs show invalid connection settings and never offer
Reconnect. A syntactically valid board link without the tab's private ownership
capability shows board recovery failure. Reconnect is available only when an
existing connection can actually retry; a public URL cannot recover ownership.

`view=board` without a match is the launcher creation route and requires a valid
`launch=moenarch` and `session` pair as described in [launch links](moenarch-launch.md).
It is not a standalone creation shortcut. Canonical match links, including
same-tab Return to board, continue to use the tab's existing board capability
without requiring launch context. The parameter remains named `view`.

Board recovery, invalid/unavailable invites, unrecognized views and failed share
links offer **Create a new game**. This opens the existing setup flow, removing
`match`, `view`, `share`, `launch`, `session`, `fixture` and the fragment from the
current address while preserving language, connection settings and unrelated
parameters. Opening setup does not create a match. Only its normal **Create game**
action does that. Recovery leaves private board/player capabilities in their
existing tab-owned stores; it neither copies them into links nor grants ownership
of the failed match. Back returns to the explicit error and forward returns to
setup; reload preserves each URL's meaning.

Layout and word-list shares require an `s_` prefix followed by 32 lowercase hex
digits. Malformed or missing share IDs show an invalid-share-link error before
fetching the resource; a validly formed ID whose resource is withdrawn, expired
or otherwise unavailable retains its distinct unavailable message.

The independent `ROUTE-NAV` expected-state model intentionally replaces its
unknown-view creation fallback and malformed-share unavailable expectation with
these errors. `ROUTE-RECOVERY` adds bounded, hand-authored paths for error reload,
recovery, setup reload, back/forward, actual creation and board reload, using real
Rust fixtures. The same recovery paths run against the built client in the Pages
suite. These finite paths cover the named cases, not arbitrary URL combinations;
full graph exploration remains opt-in.
