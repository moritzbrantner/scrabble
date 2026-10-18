/**
 * Player-local physical arrangement of rack tiles. It only changes display order and never
 * a tile's identity, count, value or the authoritative rack.
 */
export function arrangeRack<T extends { id: string }>(
  tiles: readonly T[],
  order: readonly string[],
): T[] {
  const byId = new Map(tiles.map((tile) => [tile.id, tile]));
  const arranged: T[] = [];
  const seen = new Set<string>();
  for (const id of order) {
    const tile = byId.get(id);
    if (tile !== undefined && !seen.has(id)) {
      arranged.push(tile);
      seen.add(id);
    }
  }
  // Refilled tiles join at the end in authoritative order.
  for (const tile of tiles) {
    if (!seen.has(tile.id)) {
      arranged.push(tile);
    }
  }
  return arranged;
}

/** Moves one tile so it occupies `target` in the resulting order (clamped to the rack). */
export function moveInOrder(order: readonly string[], tileId: string, target: number): string[] {
  if (!order.includes(tileId)) {
    return [...order];
  }
  const without = order.filter((id) => id !== tileId);
  const index = Math.max(0, Math.min(target, without.length));
  return [...without.slice(0, index), tileId, ...without.slice(index)];
}
