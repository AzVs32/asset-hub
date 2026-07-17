import Form, { type FormProps } from "@rjsf/core";
import validator from "@rjsf/validator-ajv8";
import React from "react";
import type {
  PluginActionOutput,
  Resource,
  ResourceActionDefinition,
} from "../../api/contracts";
import { primaryButtonClass } from "../../components/ui";
import {
  executeResourceAction,
  findAvailableAction,
} from "../host/actions";
import type { JsonObject, PluginView } from "../host/contracts";

export default function JsonSchemaFormView({
  view,
  resource,
  action,
  onExecuted,
}: {
  view: Extract<PluginView, { view: "form" }>;
  resource: Resource;
  action: string;
  onExecuted: (
    output: PluginActionOutput,
    targetAction: ResourceActionDefinition,
  ) => void | Promise<void>;
}) {
  const targetActionId = view.submit_action || action;
  const targetAction = findAvailableAction(resource, targetActionId);
  const [status, setStatus] = React.useState("");

  if (!targetAction) {
    return <div className="min-h-40 bg-slate-50 p-5 text-sm text-red-700">Form action is not available</div>;
  }

  return (
    <Form
      className="rjsf"
      schema={view.schema as FormProps["schema"]}
      formData={view.value}
      validator={validator}
      showErrorList="top"
      onSubmit={async ({ formData }) => {
        setStatus("Submitting");
        try {
          const output = await executeResourceAction(
            resource,
            targetAction.id,
            asJsonObject(formData),
          );
          setStatus("Submitted");
          await onExecuted(output, targetAction);
        } catch (error) {
          setStatus(error instanceof Error ? error.message : "Submit failed");
        }
      }}
    >
      <div className="flex items-center gap-3">
        <button className={primaryButtonClass} type="submit">Submit</button>
        <span className="text-sm text-slate-500">{status}</span>
      </div>
    </Form>
  );
}

function asJsonObject(value: unknown): JsonObject {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? value as JsonObject
    : {};
}

