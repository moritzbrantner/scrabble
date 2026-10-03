# Scrabble

A match has one shared board and each player has a private rack. Players take turns forming words under one selected ruleset.

## Language

**Match**: One lobby, playing session, and finished result shared by a group of players. A rematch is a new match.
_Avoid_: Room when referring to a completed or in-progress match

**Rack**: The tiles privately held by one player, available for their next placement or exchange.
_Avoid_: Hand, inventory

**Bag**: The remaining tiles from which racks are filled; its order and contents are hidden.

**Proposed placement**: A player's chosen tile identities, board coordinates, and blank assignments submitted for validation.
_Avoid_: Move when the tiles have not yet been accepted

**Preview**: A temporary public view of proposed tiles during the active turn, with no score or committed board effect.
_Avoid_: Committed placement, optimistic board

**Move**: An accepted placement committed to the board with its score and turn advancement.

**Ruleset**: The immutable configuration of tile distribution, values, board premiums, turn limits, and dictionary identity selected for a match.

**Blank assignment**: The letter represented by a blank tile; the tile retains its zero value.
