import { afterEach } from "vitest";
import { cleanup } from "@testing-library/preact";

// Without vitest globals, @testing-library/preact does not unmount between tests on its own.
afterEach(cleanup);
