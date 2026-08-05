import { Save } from "lucide-react";
import React from "react";
import { useGateway } from "@/application/ports/gateway-context";
import type { Resource } from "@/domain/resource";
import { Button } from "@/shared/ui/button";
import { Textarea } from "@/shared/ui/field";

export function CoreTextEditor({
  resource,
  initialText,
  onSaved,
  onClose,
}: {
  resource: Resource;
  initialText: string;
  onSaved: () => void | Promise<void>;
  onClose: () => void;
}) {
  const gateway = useGateway();
  const [text, setText] = React.useState(initialText);
  const [saving, setSaving] = React.useState(false);
  const [error, setError] = React.useState<string | null>(null);

  async function save(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    setSaving(true);
    setError(null);
    try {
      await gateway.replaceResourceText(resource, text);
      await onSaved();
      onClose();
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : "Unable to save text");
    } finally {
      setSaving(false);
    }
  }

  return (
    <form className="grid gap-4 p-6" onSubmit={save}>
      <Textarea
        aria-label="Text content"
        className="min-h-[50vh] font-mono leading-6"
        value={text}
        disabled={saving}
        onChange={(event) => setText(event.target.value)}
      />
      {error ? (
        <p className="text-sm text-red-700" role="alert">
          {error}
        </p>
      ) : null}
      <div className="flex justify-end">
        <Button disabled={saving} type="submit">
          <Save size={16} />
          Save
        </Button>
      </div>
    </form>
  );
}
