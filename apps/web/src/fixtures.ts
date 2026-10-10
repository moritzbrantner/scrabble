import { z } from "zod";

import data from "./fixtures/public.json";
import { publicSnapshot, ruleset } from "./public-state";
export const fixtures = z
  .strictObject({
    ruleset,
    snapshots: z.strictObject({
      lobby: publicSnapshot,
      playing: publicSnapshot,
      finished: publicSnapshot,
    }),
  })
  .parse(data);
