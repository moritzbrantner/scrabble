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
