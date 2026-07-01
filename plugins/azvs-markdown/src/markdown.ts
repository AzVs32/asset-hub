export interface PluginActionRequest {
  action: string;
  input?: unknown;
  resource: PluginResource;
  content?: PluginContentBytes;
}

export interface PluginResource {
  id: string;
  name: string;
  kind: string;
  status: string;
  metadata?: unknown;
  content?: {
    key: string;
    size: number;
    mime_type?: string;
    original_filename?: string;
    checksum?: Array<{ kind: string; value: string }>;
  };
  created_at: string;
  updated_at: string;
  deleted_at?: string | null;
}

export interface PluginContentBytes {
  encoding: "base64";
  data: string;
}

export interface MarkdownHeading {
  id: string;
  level: number;
  title: string;
  line: number;
  children: MarkdownHeading[];
}

export interface MarkdownSection {
  id: string;
  heading_id: string | null;
  title: string;
  level: number;
  start_line: number;
  end_line: number;
  markdown: string;
}

export interface MarkdownBlock {
  type: "heading" | "paragraph" | "list" | "blockquote" | "code" | "thematic_break" | "table";
  start_line: number;
  end_line: number;
  id?: string;
  level?: number;
  text?: string;
  markdown?: string;
  language?: string;
  ordered?: boolean;
}

export interface MarkdownViewDescription {
  schema: "azvs.markdown.view@1";
  kind: "core:document";
  parent_kind: "core:document";
  resource: {
    id: string;
    name: string;
    original_filename: string | null;
    mime_type: string | null;
  };
  metadata: {
    title: string;
    description: string | null;
    front_matter: Record<string, unknown>;
    front_matter_raw: string | null;
    line_count: number;
    word_count: number;
    heading_count: number;
    section_count: number;
  };
  headings: MarkdownHeading[];
  sections: Record<string, MarkdownSection>;
  blocks: MarkdownBlock[];
  source: {
    markdown: string;
  };
}

interface FlatHeading {
  id: string;
  level: number;
  title: string;
  line: number;
}

interface FrontMatterResult {
  data: Record<string, unknown>;
  raw: string | null;
  bodyStartLine: number;
}

export function buildMarkdownView(request: PluginActionRequest): MarkdownViewDescription {
  if (!request.content) {
    throw new Error("missing Markdown content payload");
  }
  if (request.content.encoding !== "base64") {
    throw new Error("unsupported content encoding");
  }

  const markdown = decodeBase64Utf8(request.content.data).replace(/^\uFEFF/, "");
  const parsed = parseMarkdown(markdown);
  const firstHeading = parsed.flatHeadings.find((heading) => heading.level === 1) ?? parsed.flatHeadings[0];
  const frontMatterTitle = stringMetadata(parsed.frontMatter.data.title);
  const resourceTitle = stringMetadata(kindMetadata(request.resource.metadata).title);
  const title = frontMatterTitle || resourceTitle || firstHeading?.title || request.resource.name;
  const description =
    stringMetadata(parsed.frontMatter.data.description) ||
    stringMetadata(kindMetadata(request.resource.metadata).description) ||
    null;

  return {
    schema: "azvs.markdown.view@1",
    kind: "core:document",
    parent_kind: "core:document",
    resource: {
      id: request.resource.id,
      name: request.resource.name,
      original_filename: request.resource.content?.original_filename ?? null,
      mime_type: request.resource.content?.mime_type ?? null,
    },
    metadata: {
      title,
      description,
      front_matter: parsed.frontMatter.data,
      front_matter_raw: parsed.frontMatter.raw,
      line_count: markdown.length === 0 ? 0 : splitLines(markdown).length,
      word_count: countWords(markdown),
      heading_count: parsed.flatHeadings.length,
      section_count: Object.keys(parsed.sections).length,
    },
    headings: parsed.headingTree,
    sections: parsed.sections,
    blocks: parsed.blocks,
    source: {
      markdown,
    },
  };
}

export function parseMarkdown(markdown: string): {
  frontMatter: FrontMatterResult;
  flatHeadings: FlatHeading[];
  headingTree: MarkdownHeading[];
  sections: Record<string, MarkdownSection>;
  blocks: MarkdownBlock[];
} {
  const frontMatter = parseFrontMatter(markdown);
  const lines = splitLines(markdown);
  const flatHeadings: FlatHeading[] = [];
  const blocks: MarkdownBlock[] = [];
  const slugCounts = new Map<string, number>();
  let fence: { marker: string; length: number; startLine: number; language: string; lines: string[] } | null = null;
  let paragraphStart: number | null = null;
  let paragraphLines: string[] = [];

  const flushParagraph = (endLine: number): void => {
    if (paragraphStart === null || paragraphLines.length === 0) {
      paragraphStart = null;
      paragraphLines = [];
      return;
    }
    blocks.push({
      type: "paragraph",
      start_line: paragraphStart,
      end_line: endLine,
      text: cleanInlineText(paragraphLines.join(" ")),
      markdown: paragraphLines.join("\n"),
    });
    paragraphStart = null;
    paragraphLines = [];
  };

  for (let index = frontMatter.bodyStartLine - 1; index < lines.length; index += 1) {
    const line = lines[index];
    const lineNumber = index + 1;
    const fenceStart = line.match(/^ {0,3}(`{3,}|~{3,})(.*)$/);

    if (fence) {
      fence.lines.push(line);
      if (fenceStart && fenceStart[1][0] === fence.marker && fenceStart[1].length >= fence.length) {
        blocks.push({
          type: "code",
          start_line: fence.startLine,
          end_line: lineNumber,
          language: fence.language,
          markdown: fence.lines.join("\n"),
        });
        fence = null;
      }
      continue;
    }

    if (fenceStart) {
      flushParagraph(lineNumber - 1);
      fence = {
        marker: fenceStart[1][0],
        length: fenceStart[1].length,
        startLine: lineNumber,
        language: fenceStart[2].trim().split(/\s+/)[0] ?? "",
        lines: [line],
      };
      continue;
    }

    const atx = parseAtxHeading(line);
    if (atx) {
      flushParagraph(lineNumber - 1);
      const id = uniqueSlug(atx.title, slugCounts);
      flatHeadings.push({ id, level: atx.level, title: atx.title, line: lineNumber });
      blocks.push({
        type: "heading",
        id,
        level: atx.level,
        start_line: lineNumber,
        end_line: lineNumber,
        text: atx.title,
        markdown: line,
      });
      continue;
    }

    const nextLine = lines[index + 1];
    const setext = parseSetextHeading(paragraphLines, nextLine);
    if (setext && paragraphStart !== null) {
      const title = cleanInlineText(paragraphLines.join(" "));
      const id = uniqueSlug(title, slugCounts);
      flatHeadings.push({ id, level: setext.level, title, line: paragraphStart });
      blocks.push({
        type: "heading",
        id,
        level: setext.level,
        start_line: paragraphStart,
        end_line: lineNumber + 1,
        text: title,
        markdown: [...paragraphLines, nextLine].join("\n"),
      });
      paragraphStart = null;
      paragraphLines = [];
      index += 1;
      continue;
    }

    if (/^\s*$/.test(line)) {
      flushParagraph(lineNumber - 1);
      continue;
    }

    const thematicBreak = /^ {0,3}((\*\s*){3,}|(-\s*){3,}|(_\s*){3,})$/.test(line.trim());
    if (thematicBreak) {
      flushParagraph(lineNumber - 1);
      blocks.push({
        type: "thematic_break",
        start_line: lineNumber,
        end_line: lineNumber,
        markdown: line,
      });
      continue;
    }

    const blockType = classifyLineBlock(line);
    if (blockType) {
      flushParagraph(lineNumber - 1);
      const previous = blocks[blocks.length - 1];
      if (
        previous &&
        previous.type === blockType.type &&
        previous.end_line === lineNumber - 1 &&
        previous.ordered === blockType.ordered
      ) {
        previous.end_line = lineNumber;
        previous.markdown = `${previous.markdown ?? ""}\n${line}`;
      } else {
        blocks.push({
          type: blockType.type,
          ordered: blockType.ordered,
          start_line: lineNumber,
          end_line: lineNumber,
          markdown: line,
        });
      }
      continue;
    }

    if (paragraphStart === null) {
      paragraphStart = lineNumber;
    }
    paragraphLines.push(line);
  }

  if (fence) {
    blocks.push({
      type: "code",
      start_line: fence.startLine,
      end_line: lines.length,
      language: fence.language,
      markdown: fence.lines.join("\n"),
    });
  }
  flushParagraph(lines.length);

  return {
    frontMatter,
    flatHeadings,
    headingTree: buildHeadingTree(flatHeadings),
    sections: buildSections(lines, frontMatter.bodyStartLine, flatHeadings),
    blocks,
  };
}

function parseFrontMatter(markdown: string): FrontMatterResult {
  const lines = splitLines(markdown);
  if (lines[0]?.trim() !== "---") {
    return { data: {}, raw: null, bodyStartLine: 1 };
  }

  for (let index = 1; index < lines.length; index += 1) {
    const trimmed = lines[index].trim();
    if (trimmed === "---" || trimmed === "...") {
      const rawLines = lines.slice(1, index);
      return {
        data: parseSimpleYaml(rawLines),
        raw: rawLines.join("\n"),
        bodyStartLine: index + 2,
      };
    }
  }

  return { data: {}, raw: null, bodyStartLine: 1 };
}

function parseSimpleYaml(lines: string[]): Record<string, unknown> {
  const data: Record<string, unknown> = {};
  let currentKey: string | null = null;

  for (const line of lines) {
    if (/^\s*$/.test(line) || /^\s*#/.test(line)) {
      continue;
    }
    const listItem = line.match(/^\s*-\s+(.+)$/);
    if (listItem && currentKey) {
      const current = data[currentKey];
      if (Array.isArray(current)) {
        current.push(parseScalar(listItem[1]));
      }
      continue;
    }

    const entry = line.match(/^([A-Za-z0-9_.-]+):(?:\s*(.*))?$/);
    if (!entry) {
      currentKey = null;
      continue;
    }

    currentKey = entry[1];
    const rawValue = entry[2] ?? "";
    data[currentKey] = rawValue === "" ? [] : parseScalar(rawValue);
  }

  return data;
}

function parseScalar(value: string): unknown {
  const trimmed = value.trim();
  if ((trimmed.startsWith('"') && trimmed.endsWith('"')) || (trimmed.startsWith("'") && trimmed.endsWith("'"))) {
    return trimmed.slice(1, -1);
  }
  if (trimmed === "true") return true;
  if (trimmed === "false") return false;
  if (trimmed === "null") return null;
  if (/^-?\d+(\.\d+)?$/.test(trimmed)) return Number(trimmed);
  if (trimmed.startsWith("[") && trimmed.endsWith("]")) {
    return trimmed
      .slice(1, -1)
      .split(",")
      .map((item) => parseScalar(item))
      .filter((item) => item !== "");
  }
  return trimmed;
}

function parseAtxHeading(line: string): { level: number; title: string } | null {
  const match = line.match(/^ {0,3}(#{1,6})(?:\s+|$)(.*?)\s*#*\s*$/);
  if (!match) return null;
  const title = cleanInlineText(match[2]);
  return title ? { level: match[1].length, title } : null;
}

function parseSetextHeading(paragraphLines: string[], nextLine: string | undefined): { level: 1 | 2 } | null {
  if (paragraphLines.length !== 1 || nextLine === undefined) return null;
  if (/^ {0,3}=+\s*$/.test(nextLine)) return { level: 1 };
  if (/^ {0,3}-+\s*$/.test(nextLine)) return { level: 2 };
  return null;
}

function classifyLineBlock(
  line: string,
): { type: "list" | "blockquote" | "table"; ordered?: boolean } | null {
  if (/^ {0,3}>\s?/.test(line)) return { type: "blockquote" };
  if (/^ {0,3}[-+*]\s+/.test(line)) return { type: "list", ordered: false };
  if (/^ {0,3}\d+[.)]\s+/.test(line)) return { type: "list", ordered: true };
  if (line.includes("|")) return { type: "table" };
  return null;
}

function buildHeadingTree(flatHeadings: FlatHeading[]): MarkdownHeading[] {
  const root: MarkdownHeading[] = [];
  const stack: MarkdownHeading[] = [];

  for (const heading of flatHeadings) {
    const node: MarkdownHeading = { ...heading, children: [] };
    while (stack.length > 0 && stack[stack.length - 1].level >= node.level) {
      stack.pop();
    }
    const parent = stack[stack.length - 1];
    if (parent) {
      parent.children.push(node);
    } else {
      root.push(node);
    }
    stack.push(node);
  }

  return root;
}

function buildSections(
  lines: string[],
  bodyStartLine: number,
  headings: FlatHeading[],
): Record<string, MarkdownSection> {
  const sections: Record<string, MarkdownSection> = {};

  if (headings.length === 0) {
    const markdown = lines.slice(bodyStartLine - 1).join("\n").trim();
    sections.root = {
      id: "root",
      heading_id: null,
      title: "Document",
      level: 0,
      start_line: bodyStartLine,
      end_line: lines.length,
      markdown,
    };
    return sections;
  }

  for (let index = 0; index < headings.length; index += 1) {
    const heading = headings[index];
    const next = headings[index + 1];
    const startLine = heading.line + 1;
    const endLine = (next?.line ?? lines.length + 1) - 1;
    sections[heading.id] = {
      id: heading.id,
      heading_id: heading.id,
      title: heading.title,
      level: heading.level,
      start_line: startLine,
      end_line: Math.max(startLine, endLine),
      markdown: lines.slice(startLine - 1, endLine).join("\n").trim(),
    };
  }

  return sections;
}

function uniqueSlug(title: string, counts: Map<string, number>): string {
  const base = slugify(title) || "section";
  const count = counts.get(base) ?? 0;
  counts.set(base, count + 1);
  return count === 0 ? base : `${base}-${count + 1}`;
}

function slugify(value: string): string {
  return value
    .toLowerCase()
    .normalize("NFKD")
    .replace(/[\u0300-\u036f]/g, "")
    .replace(/[`*_~[\](){}<>.!?:;"',\\/|+=#$%^&@]+/g, "")
    .replace(/\s+/g, "-")
    .replace(/-+/g, "-")
    .replace(/^-|-$/g, "");
}

function cleanInlineText(value: string): string {
  return value
    .replace(/!\[([^\]]*)]\([^)]+\)/g, "$1")
    .replace(/\[([^\]]+)]\([^)]+\)/g, "$1")
    .replace(/[`*_~]/g, "")
    .replace(/\s+/g, " ")
    .trim();
}

function countWords(markdown: string): number {
  const text = markdown
    .replace(/^---[\s\S]*?\n(?:---|\.\.\.)/m, " ")
    .replace(/```[\s\S]*?```/g, " ")
    .replace(/~~~[\s\S]*?~~~/g, " ")
    .replace(/[#>*_`~[\]()|.!?:;"',\\/+=-]+/g, " ");
  return text.trim() ? text.trim().split(/\s+/).length : 0;
}

function splitLines(value: string): string[] {
  return value.replace(/\r\n?/g, "\n").split("\n");
}

function kindMetadata(metadata: unknown): Record<string, unknown> {
  if (!isRecord(metadata)) return {};
  const kind = metadata.kind;
  if (!isRecord(kind)) return {};
  return isRecord(kind.data) ? kind.data : {};
}

function stringMetadata(value: unknown): string | null {
  return typeof value === "string" && value.trim() ? value.trim() : null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function decodeBase64Utf8(base64: string): string {
  const chars = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
  const clean = base64.replace(/^data:[^,]+,/, "").replace(/\s+/g, "");
  const bytes: number[] = [];

  for (let index = 0; index < clean.length; index += 4) {
    const chunk = clean.slice(index, index + 4);
    const values = [...chunk].map((char) => (char === "=" ? 0 : chars.indexOf(char)));
    if (values.some((value) => value < 0)) {
      throw new Error("invalid base64 content");
    }
    const triple = (values[0] << 18) | (values[1] << 12) | (values[2] << 6) | values[3];
    bytes.push((triple >> 16) & 0xff);
    if (chunk[2] !== "=") bytes.push((triple >> 8) & 0xff);
    if (chunk[3] !== "=") bytes.push(triple & 0xff);
  }

  return decodeUtf8(bytes);
}

function decodeUtf8(bytes: number[]): string {
  let result = "";
  for (let index = 0; index < bytes.length; index += 1) {
    const byte = bytes[index];
    if (byte < 0x80) {
      result += String.fromCharCode(byte);
    } else if (byte >= 0xc0 && byte < 0xe0) {
      const next = bytes[++index];
      result += String.fromCharCode(((byte & 0x1f) << 6) | (next & 0x3f));
    } else if (byte >= 0xe0 && byte < 0xf0) {
      const b2 = bytes[++index];
      const b3 = bytes[++index];
      result += String.fromCharCode(((byte & 0x0f) << 12) | ((b2 & 0x3f) << 6) | (b3 & 0x3f));
    } else {
      const b2 = bytes[++index];
      const b3 = bytes[++index];
      const b4 = bytes[++index];
      const codePoint = ((byte & 0x07) << 18) | ((b2 & 0x3f) << 12) | ((b3 & 0x3f) << 6) | (b4 & 0x3f);
      const adjusted = codePoint - 0x10000;
      result += String.fromCharCode(0xd800 + (adjusted >> 10), 0xdc00 + (adjusted & 0x3ff));
    }
  }
  return result;
}
