import Form, { type FormProps } from "@rjsf/core";
import validator from "@rjsf/validator-ajv8";
import React from "react";
import { useWorkspaceScope } from "@/application/workspace/workspace-scope-context";
import type { JsonObject } from "@/domain/plugin";
import type { PluginViewRendererProps } from "@/kernel/plugin-kernel";
import { PluginOutput } from "@/plugins/plugin-output";
import { Button } from "@/shared/ui/button";

export default function FormRenderer({
  view,
  output,
  resource,
  gateway,
  onResourceChanged,
}: PluginViewRendererProps) {
  const scope = useWorkspaceScope();
  const [result, setResult] = React.useState<Awaited<
    ReturnType<typeof gateway.executeAction>
  > | null>(null);
  const [error, setError] = React.useState<string | null>(null);
  const [pending, setPending] = React.useState(false);
  if (view.view !== "form") return null;
  const targetId = view.submit_action ?? output.action;
  const target = resource.actions.find((action) => action.id === targetId);
  if (!target)
    return (
      <p className="m-4 rounded-xl bg-red-50 p-4 text-sm text-red-700">
        Action {targetId} is not available for this resource.
      </p>
    );
  return (
    <div className="grid gap-5 p-6">
      <Form
        className="rjsf"
        schema={view.schema as FormProps["schema"]}
        formData={view.value}
        validator={validator}
        showErrorList="top"
        disabled={pending}
        onSubmit={async ({ formData }) => {
          setPending(true);
          setError(null);
          try {
            const next = await gateway.executeAction(
              scope.toStorageResource(resource),
              target.id,
              jsonObject(formData),
            );
            setResult(next);
            if (target.access === "read_write") await onResourceChanged?.();
          } catch (cause) {
            setError(cause instanceof Error ? cause.message : "Plugin form submission failed");
          } finally {
            setPending(false);
          }
        }}
      >
        <Button type="submit" disabled={pending}>
          {pending ? "Submitting…" : "Submit"}
        </Button>
      </Form>
      {error ? <p className="rounded-xl bg-red-50 p-4 text-sm text-red-700">{error}</p> : null}
      {result ? (
        <PluginOutput output={result} resource={resource} onResourceChanged={onResourceChanged} />
      ) : null}
    </div>
  );
}

function jsonObject(value: unknown): JsonObject {
  return value && typeof value === "object" && !Array.isArray(value) ? (value as JsonObject) : {};
}
