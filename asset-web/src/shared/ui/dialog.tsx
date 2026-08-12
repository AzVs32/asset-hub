import * as DialogPrimitive from "@radix-ui/react-dialog";
import { X } from "lucide-react";
import type React from "react";
import { Button } from "./button";
import { cn } from "./cn";

export function Dialog({
  open,
  onOpenChange,
  title,
  description,
  children,
  className,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: string;
  description?: string | undefined;
  children: React.ReactNode;
  className?: string | undefined;
}) {
  return (
    <DialogPrimitive.Root open={open} onOpenChange={onOpenChange}>
      <DialogPrimitive.Portal>
        <DialogPrimitive.Overlay className="fixed inset-0 z-40 bg-slate-950/60 backdrop-blur-md" />
        <DialogPrimitive.Content
          className={cn(
            "dialog-surface fixed left-1/2 top-1/2 z-50 max-h-[calc(100vh-2rem)] w-[calc(100%-2rem)] max-w-xl -translate-x-1/2 -translate-y-1/2 overflow-auto rounded-3xl border border-white/70 bg-white shadow-[0_32px_100px_-28px_rgba(15,23,42,0.65)]",
            className,
          )}
        >
          <header className="sticky top-0 z-10 flex items-start justify-between border-b border-slate-100 bg-white/95 px-6 py-5 backdrop-blur-xl">
            <div>
              <DialogPrimitive.Title className="text-xl font-bold tracking-[-0.025em] text-slate-950">
                {title}
              </DialogPrimitive.Title>
              {description ? (
                <DialogPrimitive.Description className="mt-1 text-sm text-slate-500">
                  {description}
                </DialogPrimitive.Description>
              ) : null}
            </div>
            <DialogPrimitive.Close asChild>
              <Button variant="ghost" size="icon" aria-label="Close dialog">
                <X size={18} />
              </Button>
            </DialogPrimitive.Close>
          </header>
          {children}
        </DialogPrimitive.Content>
      </DialogPrimitive.Portal>
    </DialogPrimitive.Root>
  );
}
