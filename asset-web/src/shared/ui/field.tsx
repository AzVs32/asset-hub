import React from "react";
import { cn } from "./cn";

export const controlClass =
  "min-h-10 w-full rounded-lg border border-slate-300 bg-white px-3 py-2 text-sm text-slate-900 outline-none placeholder:text-slate-400 focus:border-blue-500 focus:ring-4 focus:ring-blue-100 disabled:bg-slate-100";

export function Field({
  label,
  error,
  children,
}: {
  label: string;
  error?: string | undefined;
  children: React.ReactElement<{
    id?: string | undefined;
    "aria-describedby"?: string | undefined;
  }>;
}) {
  const id = React.useId();
  const errorId = `${id}-error`;
  return (
    <div className="grid gap-1.5 text-sm font-medium text-slate-700">
      <label htmlFor={id}>{label}</label>
      {React.cloneElement(children, { id, ...(error ? { "aria-describedby": errorId } : {}) })}
      {error ? (
        <span id={errorId} className="text-xs text-red-600">
          {error}
        </span>
      ) : null}
    </div>
  );
}

export const Input = React.forwardRef<
  HTMLInputElement,
  React.InputHTMLAttributes<HTMLInputElement>
>(({ className, ...props }, ref) => (
  <input ref={ref} className={cn(controlClass, className)} {...props} />
));
Input.displayName = "Input";

export const Textarea = React.forwardRef<
  HTMLTextAreaElement,
  React.TextareaHTMLAttributes<HTMLTextAreaElement>
>(({ className, ...props }, ref) => (
  <textarea ref={ref} className={cn(controlClass, "min-h-24 resize-y", className)} {...props} />
));
Textarea.displayName = "Textarea";
