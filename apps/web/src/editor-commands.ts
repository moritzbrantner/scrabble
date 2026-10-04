/** Shared dispatch and help entries for board navigation; placement also uses native Enter/Space. */
export const boardCommands = [
  {
    key: "ArrowLeft",
    label: "command.left",
    target: (index: number, size: number) => index - (index % size > 0 ? 1 : 0),
  },
  {
    key: "ArrowRight",
    label: "command.right",
    target: (index: number, size: number) => index + (index % size < size - 1 ? 1 : 0),
  },
  {
    key: "ArrowUp",
    label: "command.up",
    target: (index: number, size: number) => Math.max(index % size, index - size),
  },
  {
    key: "ArrowDown",
    label: "command.down",
    target: (index: number, size: number) =>
      Math.min(size * (size - 1) + (index % size), index + size),
  },
  {
    key: "Home",
    label: "command.first",
    target: (index: number, size: number) => index - (index % size),
  },
  {
    key: "End",
    label: "command.last",
    target: (index: number, size: number) => index - (index % size) + size - 1,
  },
] as const;
