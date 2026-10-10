import { createCm } from "./cm-engine";
import { start } from "./harness";
start(createCm((o) => window.ipc.postMessage(JSON.stringify(o))));
