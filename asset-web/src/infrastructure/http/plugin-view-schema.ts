import { z } from "zod";
import { type JsonValue, type PluginView, pluginViewKinds } from "@/domain/plugin";

const jsonValueSchema: z.ZodType<JsonValue> = z.lazy(() =>
  z.union([
    z.null(),
    z.boolean(),
    z.number(),
    z.string(),
    z.array(jsonValueSchema),
    z.record(z.string(), jsonValueSchema),
  ]),
);

const pluginViewSchema = z.discriminatedUnion("view", [
  z.object({ view: z.literal("text"), text: z.string() }),
  z.object({ view: z.literal("markdown"), markdown: z.string() }),
  z.object({ view: z.literal("html"), title: z.string().optional(), html: z.string() }),
  z.object({
    view: z.literal("plugin_frame"),
    plugin_api: z.string().min(1),
    title: z.string().optional(),
    url: z.string(),
  }),
  z.object({ view: z.literal("json"), data: jsonValueSchema }),
  z.object({
    view: z.literal("media"),
    mime_type: z.string(),
    title: z.string().optional(),
    encoding: z.enum(["base64", "url"]),
    data: z.string(),
  }),
  z.object({
    view: z.literal("download"),
    url: z.string(),
    mime_type: z.string().optional(),
    filename: z.string().optional(),
  }),
]);

export function parsePluginView(value: unknown): PluginView {
  return pluginViewSchema.parse(value) as PluginView;
}

export function parseOptionalPluginView(value: unknown): PluginView | null {
  return value == null ? null : parsePluginView(value);
}

export function isPluginViewKind(value: string): value is (typeof pluginViewKinds)[number] {
  return pluginViewKinds.some((kind) => kind === value);
}
