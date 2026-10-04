# Tentative move preview

The active phone publishes only its selected placement IDs, coordinates, and assigned blank letters. The native session checks authenticated identity, sequence, active turn, rack ownership, placement count, coordinates, and blank assignments before deriving public letters. Unplaced rack letters never enter a preview.

The board overlays a matching player/turn preview only on empty squares, with dashed borders and accessible tentative labels. Committed tiles remain solid and take precedence. Edits replace the whole preview; cancel and commit submission publish an empty preview. Rapid edits are coalesced for 100 ms; a still-active draft refreshes every 500 ms. Delivery failures do not affect move acceptance.

Native previews expire after two seconds without refresh, covering lost cancel packets. Accepted disconnects and reconnects clear previews immediately through the upstream presentation hook. Pass clears the preview with the turn transition; commit acceptance will do the same in #19. Stale sequences cannot supersede newer packets, and wrong-turn or wrong-player commands cannot mutate state. Canonical snapshots omit preview data and deadlines.

Native tests exercise expiry, cancellation reordering, rejected packets, disconnect/reconnect fencing, and unchanged gameplay. Browser tests exercise actual phone edits reaching the shared board.
