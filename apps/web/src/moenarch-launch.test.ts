import { expect, test } from "bun:test";

import { moenarchLaunch } from "./moenarch-launch";

const session = "0123ABCDEFGH";
const match = `b_${"a".repeat(16)}_${"0".repeat(32)}`;

test("launcher URLs require a canonical session and an explicit authoritative match", () => {
  expect(moenarchLaunch(new URLSearchParams())).toEqual({ kind: "absent" });
  expect(moenarchLaunch(new URLSearchParams(`launch=moenarch&session=${session}`))).toEqual({
    kind: "unbound",
  });
  expect(
    moenarchLaunch(
      new URLSearchParams(`launch=moenarch&session=${session}&match=${match}&view=player`),
    ),
  ).toEqual({ kind: "linked", sessionId: session });
  expect(
    moenarchLaunch(new URLSearchParams(`launch=moenarch&session=${session}&match=${match}`)),
  ).toEqual({ kind: "linked", sessionId: session });
});

test("ambiguous, invalid and secret-bearing launch identities fail closed", () => {
  for (const query of [
    `session=${session}&match=${match}`,
    `launch=moenarch&match=${match}`,
    `launch=other&session=${session}&match=${match}`,
    `launch=moenarch&launch=moenarch&session=${session}&match=${match}`,
    `launch=moenarch&session=${session}&session=${session}&match=${match}`,
    `launch=moenarch&session=wrong&match=${match}`,
    `launch=moenarch&session=${session.toLowerCase()}&match=${match}`,
    `launch=moenarch&session=${session}&match=${match}&match=${match}`,
    `launch=moenarch&session=${session}&match=../other`,
    `launch=moenarch&session=${session}&match=${match}&view=player&view=player`,
    `launch=moenarch&session=${session}&match=${match}&view=unknown`,
  ]) {
    expect(moenarchLaunch(new URLSearchParams(query))).toEqual({ kind: "invalid" });
  }
});
