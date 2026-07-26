import { z } from "zod";
import { type PluginView, pluginViewKinds } from "@/domain/plugin";

const jsonObject = z.record(z.string(), z.unknown());

const pluginViewSchema = z.discriminatedUnion("view", [
  z.object({ view: z.literal("text"), text: z.string() }),
  z.object({ view: z.literal("markdown"), markdown: z.string() }),
  z.object({ view: z.literal("html"), title: z.string().optional(), html: z.string() }),
  z.object({ view: z.literal("plugin_frame"), title: z.string().optional(), url: z.string() }),
  z.object({ view: z.literal("json"), data: z.unknown() }),
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
  z.object({
    view: z.literal("table"),
    columns: z.array(z.object({ key: z.string(), label: z.string() })),
    rows: z.array(z.unknown()),
  }),
  z.object({
    view: z.literal("form"),
    schema: jsonObject,
    value: z.unknown().optional(),
    submit_action: z.string().optional(),
  }),
]);

export function parsePluginView(value: unknown): PluginView {
  return pluginViewSchema.parse(value) as PluginView;
}

export function isPluginViewKind(value: string): value is (typeof pluginViewKinds)[number] {
  return pluginViewKinds.some((kind) => kind === value);
}
