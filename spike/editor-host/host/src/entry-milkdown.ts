import { Editor, rootCtx, editorViewCtx } from "@milkdown/kit/core";
import { commonmark, paragraphSchema, headingSchema } from "@milkdown/kit/preset/commonmark";
import { history } from "@milkdown/kit/plugin/history";
import { $prose } from "@milkdown/kit/utils";
import { Plugin, PluginKey } from "@milkdown/kit/prose/state";
import { blockIdPlugin, changePlugin, startHost } from "./core";

// Milkdown has no "global attribute" API: the schema of each block node is extended individually.
const withBlockId = (schema: any) =>
  schema.extendSchema((prev: any) => (ctx: any) => {
    const base = prev(ctx);
    return { ...base, attrs: { ...(base.attrs ?? {}), blockId: { default: null } } };
  });

(async () => {
  const editor = await Editor.make()
    .config((ctx) => ctx.set(rootCtx, document.getElementById("editor")!))
    .use(commonmark)
    // extendSchema returns a NEW plugin; it must be registered after the preset to override its schema.
    .use(withBlockId(paragraphSchema))
    .use(withBlockId(headingSchema))
    .use(history)
    .use($prose(() => blockIdPlugin(Plugin, PluginKey)))
    .use($prose(() => changePlugin(Plugin, PluginKey)))
    .create();
  startHost(editor.action((ctx) => ctx.get(editorViewCtx)), "milkdown");
})();
