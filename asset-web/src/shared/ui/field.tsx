import React from "react";
import { cn } from "./cn";

export const controlClass =
  "min-h-10 w-full rounded-xl border border-slate-200 bg-slate-50/80 px-3.5 py-2 text-sm text-slate-900 shadow-[inset_0_1px_1px_rgba(15,23,42,0.03)] outline-none transition placeholder:text-slate-400 hover:border-slate-300 focus:border-indigo-400 focus:bg-white focus:ring-4 focus:ring-indigo-100 disabled:cursor-not-allowed disabled:bg-slate-100 disabled:text-slate-400";

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
    <div className="grid gap-1.5 text-xs font-semibold text-slate-600">
      <label htmlFor={id}>{label}</label>
      {React.cloneElement(children, { id, ...(error ? { "aria-describedby": errorId } : {}) })}
      {error ? (
        <span id={errorId} className="text-xs font-medium text-rose-600">
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
