import assert from "node:assert/strict";
import { test } from "node:test";

import { navigableScreens, SCREENS, screenForHash } from "../dist/router.js";

test("every scaffolded screen is present, in navigation order", () => {
  assert.deepEqual(
    SCREENS.map((screen) => screen.id),
    ["today", "calendar", "todos", "agents", "routines", "library", "insights", "review", "source", "settings"],
  );
});

test("every built screen is navigable and routable by its hash", () => {
  const built = SCREENS.filter((screen) => screen.status === "built");
  assert.ok(built.some((screen) => screen.id === "settings"));
  assert.deepEqual(navigableScreens(), built);
  for (const screen of built) {
    assert.equal(navigableScreens().includes(screen), true, `${screen.id} is navigable`);
    assert.equal(screenForHash(`#${screen.id}`), screen, `${screen.id} is routable by hash`);
  }
});

test("planned screens stay out of navigation and cannot be routed to", () => {
  const planned = SCREENS.filter((screen) => screen.status === "planned");
  // The last planned screen may be built; every remaining planned screen is still excluded.
  for (const screen of navigableScreens()) {
    assert.equal(screen.status, "built");
  }
  for (const screen of planned) {
    assert.equal(screenForHash(`#${screen.id}`), undefined);
  }
  assert.equal(screenForHash("#no-such-screen"), undefined);
  assert.equal(screenForHash(""), undefined);
});
