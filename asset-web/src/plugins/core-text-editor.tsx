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
    <form className="grid gap-4 bg-slate-50/50 p-6" onSubmit={save}>
      <Textarea
        aria-label="Text content"
        className="min-h-[50vh] bg-white font-mono leading-6 shadow-sm"
        value={text}
        disabled={saving}
        onChange={(event) => setText(event.target.value)}
      />
      {error ? (
        <p
          className="rounded-xl border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700"
          role="alert"
        >
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
