import assert from "node:assert/strict";
import { test } from "node:test";

import { navigableScreens, SCREENS, screenForHash } from "../dist/router.js";

test("every scaffolded screen is present, in navigation order", () => {
  assert.deepEqual(
    SCREENS.map((screen) => screen.id),
    ["today", "todos", "routines", "library", "insights", "review", "source"],
  );
});

test("planned screens stay out of navigation and cannot be routed to", () => {
  const planned = SCREENS.filter((screen) => screen.status === "planned");
  assert.ok(planned.length > 0, "fixture expects planned screens in the scaffold");
  for (const screen of navigableScreens()) {
    assert.equal(screen.status, "built");
  }
  for (const screen of planned) {
    assert.equal(screenForHash(`#${screen.id}`), undefined);
  }
  assert.equal(screenForHash("#no-such-screen"), undefined);
  assert.equal(screenForHash(""), undefined);
});
