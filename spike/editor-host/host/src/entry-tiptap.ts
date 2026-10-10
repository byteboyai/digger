import { Editor, Extension } from "@tiptap/core";
import StarterKit from "@tiptap/starter-kit";
import { Plugin, PluginKey } from "@tiptap/pm/state";
import { blockIdPlugin, changePlugin, startHost } from "./core";

const Digger = Extension.create({
  name: "digger",
  addGlobalAttributes() {
    return [{ types: ["paragraph", "heading"], attributes: { blockId: { default: null, keepOnSplit: true } } }];
  },
  addProseMirrorPlugins() {
    return [blockIdPlugin(Plugin, PluginKey), changePlugin(Plugin, PluginKey)];
  },
});

const editor = new Editor({
  element: document.getElementById("editor")!,
  extensions: [// StarterKit's TrailingNode appends an empty paragraph after a final heading: a block Rust never created,
  // which would reach the AST as a phantom insert. The AST has no such rule, so it is disabled.
  StarterKit.configure({ heading: { levels: [1, 2] }, trailingNode: false }), Digger],
  content: "",
});
startHost(editor.view, "tiptap");
