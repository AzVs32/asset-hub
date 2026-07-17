import { request } from "../../api";
import type {
  PluginActionOutput,
  Resource,
  ResourceActionDefinition,
} from "../../api/contracts";
import type { JsonObject, PluginViewKind } from "./contracts";

export const actionLocations = {
  resourceDetail: "resource_detail",
  contextMenu: "context_menu",
  resourceListThumbnail: "resource_list_thumbnail",
} as const;

export type ActionLocation = (typeof actionLocations)[keyof typeof actionLocations];

export const supportedActionLocations: readonly ActionLocation[] = Object.values(actionLocations);
const supportedLocations = new Set<string>(supportedActionLocations);

export function actionsAt(
  resource: Resource,
  location: ActionLocation,
  includeUnknownLocations = false,
): ResourceActionDefinition[] {
  return sortActions(resource.actions.available_actions.filter((action) => {
    if (action.ui.locations.includes(location)) return true;
    if (!includeUnknownLocations || location !== actionLocations.resourceDetail) return false;
    return action.ui.locations.length === 0
      || action.ui.locations.every((candidate) => !supportedLocations.has(candidate));
  }));
}

export function selectThumbnailAction(resource: Resource): ResourceActionDefinition | null {
  const explicit = actionsAt(resource, actionLocations.resourceListThumbnail)
    .find((action) => action.access === "read_only");
  if (explicit) return explicit;
  if (!resource.content?.mime_type?.startsWith("image/")) return null;
  return sortActions(resource.actions.available_actions.filter((action) => (
    action.access === "read_only" && action.output.view.includes("media")
  )))[0] ?? null;
}

export function actionSupportsView(
  action: ResourceActionDefinition,
  view: PluginViewKind,
): boolean {
  return action.output.view.includes(view);
}

export function findAvailableAction(
  resource: Resource,
  actionId: string,
): ResourceActionDefinition | null {
  return resource.actions.available_actions.find((action) => action.id === actionId) ?? null;
}

export function isContentFallbackAction(
  resource: Resource,
  action: ResourceActionDefinition,
): boolean {
  return Boolean(
    resource.content
    && action.executor.type === "builtin"
    && !action.executor.handler
    && action.access === "read_only"
    && action.output.view.length === 0,
  );
}

export async function executeResourceAction(
  resource: Resource,
  actionId: string,
  input: JsonObject = {},
): Promise<PluginActionOutput> {
  const action = findAvailableAction(resource, actionId);
  if (!action) {
    throw new Error(`Action ${actionId} is not available for this resource`);
  }
  if (isContentFallbackAction(resource, action)) {
    return {
      resource_id: resource.id,
      action: action.id,
      diagnostics: [],
      view: {
        view: "binary_url",
        url: `/resources/${encodeURIComponent(resource.id)}/content`,
        mime_type: resource.content?.mime_type ?? undefined,
        filename: resource.content?.original_filename ?? resource.name,
      },
    };
  }
  return request<PluginActionOutput>(
    `/resources/${encodeURIComponent(resource.id)}/actions/${encodeURIComponent(actionId)}`,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ input }),
    },
  );
}

function sortActions(actions: ResourceActionDefinition[]): ResourceActionDefinition[] {
  return [...actions].sort((left, right) => (
    (left.ui.group ?? "").localeCompare(right.ui.group ?? "")
    || (left.ui.order ?? 0) - (right.ui.order ?? 0)
    || left.label.localeCompare(right.label)
  ));
}
