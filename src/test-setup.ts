import { cleanup } from "@testing-library/react";
import { afterEach } from "vitest";

// Vitest runs without globals, so Testing Library's automatic DOM cleanup
// never registers. Unmount rendered trees after every test explicitly.
afterEach(() => {
  cleanup();
});
