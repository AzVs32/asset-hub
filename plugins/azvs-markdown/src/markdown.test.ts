import assert from "node:assert/strict";
import test from "node:test";
import { buildMarkdownView, decodeBase64Utf8, parseMarkdown, type PluginActionRequest } from "./markdown.js";

test("decodes UTF-8 base64 content", () => {
  assert.equal(decodeBase64Utf8("5qCH6aKY"), "标题");
});

test("parses front matter, heading tree, sections, and blocks", () => {
  const markdown = [
    "---",
    "title: Demo Doc",
    "tags: [alpha, beta]",
    "---",
    "# Intro",
    "",
    "Opening paragraph.",
    "",
    "## Details",
    "",
    "- item one",
    "- item two",
    "",
    "```ts",
    "# not a heading",
    "```",
  ].join("\n");
  const parsed = parseMarkdown(markdown);

  assert.equal(parsed.frontMatter.data.title, "Demo Doc");
  assert.deepEqual(parsed.headingTree.map((heading) => heading.id), ["intro"]);
  assert.deepEqual(parsed.headingTree[0].children.map((heading) => heading.id), ["details"]);
  assert.equal(parsed.sections.intro.markdown, "Opening paragraph.");
  assert.equal(parsed.sections.details.markdown, "- item one\n- item two\n\n```ts\n# not a heading\n```");
  assert.equal(parsed.blocks.filter((block) => block.type === "heading").length, 2);
  assert.equal(parsed.blocks.filter((block) => block.type === "code").length, 1);
});

test("builds platform-neutral Markdown document description", () => {
  const markdown = "# Title\n\nBody text.";
  const request: PluginActionRequest = {
    action: "azvs:render_markdown",
    resource: {
      id: "01900000-0000-7000-8000-000000000000",
      name: "note.md",
      kind: "core:document",
      status: "active",
      metadata: {},
      content: {
        key: "docs/note.md",
        size: markdown.length,
        mime_type: "text/markdown",
        original_filename: "note.md",
      },
      created_at: "2026-01-01T00:00:00Z",
      updated_at: "2026-01-01T00:00:00Z",
    },
    content: {
      encoding: "base64",
      data: Buffer.from(markdown, "utf8").toString("base64"),
    },
  };

  const view = buildMarkdownView(request);

  assert.equal(view.schema, "azvs.markdown.view@1");
  assert.equal(view.kind, "core:document");
  assert.equal(view.parent_kind, "core:document");
  assert.equal(view.metadata.title, "Title");
  assert.equal(view.sections.title.markdown, "Body text.");
});
