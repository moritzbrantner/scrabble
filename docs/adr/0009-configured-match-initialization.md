# Durable configured-match initialization

Accepted technical boundary for #94, Atlas 1.25/1.31/1.39. The upstream runtime
continues to own authoritative game state, replay and recovery. Scrabble owns
the immutable initialization needed to reconstruct one selected configuration.

Version-3 requests use a separate MAC domain and opaque `p_` match ID. Legacy
`g_`/`b_` requests retain their existing identity and board authority. This
explicit discriminator makes missing configured initialization an error, rather
than a silent fallback to the current deployment configuration.

Persist selected source identities, original source identities and effective
Ruleset before publishing a configured match. Store one content-addressed
normalized word set per selected content version, shared across matches. Atomic
no-overwrite publication and full fingerprint verification prevent a retry,
source edit or corrupted file from replacing a match's original inputs. No
creation nonce, seed, rack or mutable game state belongs in these manifests.
Canonical reconstruction reuses the same source constructors/composer and then
the existing simulation factory; no second game transition path is introduced.

The runtime's consumed recovery directory remains separate. Preserve its seed,
recovery and configured initialization together during deployment. Capacity is
checked before initialization; unsuccessful placement removes its fresh manifest.
Retirement removes the match manifest/cache. Immutable word versions remain
available for retained initialization and are not silently garbage-collected.

The existing version-1 rematch API still creates a new deployment-default
version-2 match, as specified by the rematch contract; this change does not
claim to select or preserve configured profiles for that separate request.
