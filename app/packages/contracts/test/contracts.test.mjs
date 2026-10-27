import assert from "node:assert/strict";
import { test } from "node:test";

test("the built contracts module loads", async () => {
  // Contracts are types only, so the compiled module is an empty namespace;
  // loading it proves the build produced a resolvable entry point that the
  // server and web packages link against.
  const contracts = await import("../dist/index.js");
  assert.equal(typeof contracts, "object");
  assert.deepEqual(Object.keys(contracts), []);
});
