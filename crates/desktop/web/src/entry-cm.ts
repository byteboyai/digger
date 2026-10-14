import { createCm } from "./cm-engine";
import { post } from "./bridge";
import { startApp } from "./app";
import { start as startSelftest } from "./harness";

const engine = createCm(post);
if (window.__DIGGER__?.mode === "selftest") startSelftest(engine);
else startApp(engine);
