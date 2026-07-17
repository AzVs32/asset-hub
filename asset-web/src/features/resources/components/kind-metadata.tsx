import Form, { type FormProps } from "@rjsf/core";
import { customizeValidator } from "@rjsf/validator-ajv8";
import Ajv2020 from "ajv/dist/2020";
import React from "react";
import type {
  JsonSchema,
  Resource,
  ResourceKind,
  ResourceKindMetadataLayer,
  ResourceKindMetadataPatch,
} from "@/domain/resource";
import { Button } from "@/shared/ui/button";

const draft202012Validator = customizeValidator({ AjvClass: Ajv2020 });

export function KindMetadataPanel({
  resource,
  kinds,
  disabled,
  onPatch,
}: {
  resource: Resource;
  kinds: ResourceKind[];
  disabled: boolean;
  onPatch: (patch: ResourceKindMetadataPatch) => Promise<unknown>;
}) {
  const lineage = metadataKindsForResource(resource, kinds);
  const definitions = lineage.filter(
    (kind): kind is ResourceKind & { metadata: NonNullable<ResourceKind["metadata"]> } =>
      kind.metadata !== null,
  );
  const knownKinds = new Set(definitions.map((kind) => kind.kind));
  const orphanedLayers = resource.metadata.kindMetadata.layers.filter(
    (layer) => !knownKinds.has(layer.kind),
  );

  if (definitions.length === 0 && orphanedLayers.length === 0) return null;

  return (
    <section className="grid gap-3" aria-labelledby="kind-metadata-heading">
      <div>
        <h3 id="kind-metadata-heading" className="text-sm font-semibold text-slate-900">
          Kind metadata
        </h3>
        <p className="mt-1 text-xs text-slate-500">
          Metadata is layered from the root kind to the resource&rsquo;s concrete kind.
        </p>
      </div>

      {definitions.map((kind) => (
        <KindMetadataLayerCard
          key={kind.kind}
          kind={kind}
          layer={resource.metadata.kindMetadata.layers.find((layer) => layer.kind === kind.kind)}
          disabled={disabled}
          onSave={(layer) => onPatch({ upsert: [layer], clear: [] })}
          onClear={() => onPatch({ upsert: [], clear: [kind.kind] })}
        />
      ))}

      {orphanedLayers.map((layer) => (
        <details key={layer.kind} className="rounded-2xl border border-amber-200 bg-amber-50 p-4">
          <summary className="cursor-pointer text-sm font-semibold text-amber-900">
            {layer.kind} · schema v{layer.schemaVersion}
          </summary>
          <p className="mt-2 text-xs text-amber-700">
            This layer has no schema in the current kind lineage and cannot be edited here.
          </p>
          <pre className="mt-3 max-h-64 overflow-auto rounded-xl bg-slate-950 p-4 text-xs text-slate-100">
            {JSON.stringify(layer.data, null, 2)}
          </pre>
        </details>
      ))}
    </section>
  );
}

function KindMetadataLayerCard({
  kind,
  layer,
  disabled,
  onSave,
  onClear,
}: {
  kind: ResourceKind & { metadata: NonNullable<ResourceKind["metadata"]> };
  layer: ResourceKindMetadataLayer | undefined;
  disabled: boolean;
  onSave: (layer: ResourceKindMetadataLayer) => Promise<unknown>;
  onClear: () => Promise<unknown>;
}) {
  const readOnly = isReadOnlyMetadataSchema(kind.metadata.schema);
  const versionMismatch =
    layer !== undefined && layer.schemaVersion !== kind.metadata.schemaVersion;
  const [formData, setFormData] = React.useState<Record<string, unknown>>(layer?.data ?? {});
  const [pending, setPending] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);
  React.useEffect(() => setFormData(layer?.data ?? {}), [layer]);
  const formDisabled = disabled || pending;

  async function apply(change: () => Promise<unknown>) {
    setPending(true);
    setError(null);
    try {
      await change();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Kind metadata update failed");
    } finally {
      setPending(false);
    }
  }

  return (
    <article className="rounded-2xl border border-slate-200 bg-white p-4">
      <header className="mb-4 flex items-start justify-between gap-3">
        <div className="min-w-0">
          <h4 className="truncate text-sm font-semibold text-slate-900">
            {schemaText(kind.metadata.schema, "title") ?? kind.label}
          </h4>
          <p className="mt-0.5 font-mono text-[11px] text-slate-400">
            {kind.kind} · schema v{kind.metadata.schemaVersion}
          </p>
        </div>
        <span className="shrink-0 rounded-full bg-slate-100 px-2 py-1 text-[10px] font-semibold uppercase tracking-wide text-slate-500">
          {versionMismatch ? "migration required" : readOnly ? "derived" : "editable"}
        </span>
      </header>

      {versionMismatch ? (
        <p className="mb-3 rounded-lg bg-amber-50 p-2 text-xs text-amber-800">
          Stored as schema v{layer.schemaVersion}; the current schema is v
          {kind.metadata.schemaVersion}. This layer remains read-only until it is migrated.
        </p>
      ) : null}

      {error ? <p className="mb-3 rounded-lg bg-red-50 p-2 text-xs text-red-700">{error}</p> : null}

      {versionMismatch ? (
        <div className="grid gap-3">
          <pre className="max-h-64 overflow-auto rounded-xl bg-slate-950 p-4 text-xs text-slate-100">
            {JSON.stringify(layer.data, null, 2)}
          </pre>
          {!readOnly ? (
            <div>
              <Button
                variant="ghost"
                size="small"
                disabled={formDisabled}
                onClick={async () => {
                  await apply(onClear);
                }}
              >
                Clear outdated layer
              </Button>
            </div>
          ) : null}
        </div>
      ) : readOnly ? (
        <SchemaMetadataView schema={kind.metadata.schema} data={layer?.data ?? {}} />
      ) : (
        <Form
          className="rjsf"
          schema={kind.metadata.schema as FormProps["schema"]}
          formData={formData}
          validator={draft202012Validator}
          showErrorList="top"
          disabled={formDisabled}
          onChange={({ formData }) => setFormData(jsonObject(formData))}
          onSubmit={async ({ formData }) => {
            await apply(() =>
              onSave({
                kind: kind.kind,
                schemaVersion: kind.metadata.schemaVersion,
                data: jsonObject(formData),
              }),
            );
          }}
        >
          <div className="flex flex-wrap gap-2">
            <Button type="submit" size="small" disabled={formDisabled}>
              {pending ? "Saving…" : `Save ${kind.label} metadata`}
            </Button>
            {layer ? (
              <Button
                variant="ghost"
                size="small"
                disabled={formDisabled}
                onClick={async () => {
                  await apply(onClear);
                }}
              >
                Clear layer
              </Button>
            ) : null}
          </div>
        </Form>
      )}
    </article>
  );
}

function SchemaMetadataView({
  schema,
  data,
}: {
  schema: JsonSchema;
  data: Record<string, unknown>;
}) {
  const resolved = resolveLocalSchema(schema, schema);
  const properties = jsonObject(resolved.properties);
  const entries = Object.entries(properties);

  if (entries.length === 0) {
    return Object.keys(data).length > 0 ? (
      <pre className="max-h-64 overflow-auto rounded-xl bg-slate-950 p-4 text-xs text-slate-100">
        {JSON.stringify(data, null, 2)}
      </pre>
    ) : (
      <p className="text-sm text-slate-400">No metadata has been extracted.</p>
    );
  }

  return (
    <dl className="grid grid-cols-2 gap-x-4 text-sm">
      {entries.map(([name, property]) => {
        const propertySchema = resolveLocalSchema(jsonObject(property), schema);
        return (
          <div key={name} className="min-w-0 border-b border-slate-100 py-2">
            <dt
              className="text-[11px] font-semibold uppercase tracking-wide text-slate-400"
              title={schemaText(propertySchema, "description") ?? undefined}
            >
              {schemaText(propertySchema, "title") ?? humanize(name)}
            </dt>
            <dd className="mt-1 break-words text-slate-700">{formatMetadataValue(data[name])}</dd>
          </div>
        );
      })}
    </dl>
  );
}

export function metadataKindsForResource(
  resource: Resource,
  kinds: ResourceKind[],
): ResourceKind[] {
  const byName = new Map(kinds.map((kind) => [kind.kind, kind]));
  const concrete = byName.get(resource.kind);
  if (!concrete) return [];
  return [...concrete.ancestors]
    .reverse()
    .concat(concrete.kind)
    .flatMap((name) => {
      const kind = byName.get(name);
      return kind ? [kind] : [];
    });
}

export function isReadOnlyMetadataSchema(schema: JsonSchema): boolean {
  return schema.readOnly === true;
}

function resolveLocalSchema(candidate: JsonSchema, root: JsonSchema): JsonSchema {
  const reference = candidate.$ref;
  if (typeof reference !== "string" || !reference.startsWith("#/")) return candidate;
  let value: unknown = root;
  for (const segment of reference
    .slice(2)
    .split("/")
    .map((part) => part.replace(/~1/g, "/").replace(/~0/g, "~"))) {
    value = jsonObject(value)[segment];
  }
  return jsonObject(value);
}

function schemaText(schema: JsonSchema, key: string): string | null {
  const value = schema[key];
  return typeof value === "string" && value.trim() ? value : null;
}

function formatMetadataValue(value: unknown): string {
  if (value === null || value === undefined || value === "") return "—";
  if (typeof value === "boolean") return value ? "Yes" : "No";
  if (typeof value === "string" || typeof value === "number") return String(value);
  if (Array.isArray(value))
    return value.length > 0 ? value.map(formatMetadataValue).join(", ") : "—";
  return JSON.stringify(value);
}

function humanize(value: string): string {
  return value.replace(/[_-]+/g, " ").replace(/^./, (letter) => letter.toUpperCase());
}

function jsonObject(value: unknown): Record<string, unknown> {
  return value && typeof value === "object" && !Array.isArray(value)
    ? (value as Record<string, unknown>)
    : {};
}
