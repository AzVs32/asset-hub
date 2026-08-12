import * as DropdownMenu from "@radix-ui/react-dropdown-menu";
import { MoreHorizontal } from "lucide-react";
import type React from "react";
import { Button } from "./button";

export function ActionMenu({ children }: { children: React.ReactNode }) {
  return (
    <DropdownMenu.Root>
      <DropdownMenu.Trigger asChild>
        <Button variant="ghost" size="icon" aria-label="Open actions">
          <MoreHorizontal size={18} />
        </Button>
      </DropdownMenu.Trigger>
      <DropdownMenu.Portal>
        <DropdownMenu.Content
          align="end"
          sideOffset={6}
          className="z-50 min-w-52 rounded-2xl border border-slate-200/80 bg-white/95 p-1.5 shadow-[0_20px_50px_-18px_rgba(15,23,42,0.35)] backdrop-blur-xl"
        >
          {children}
        </DropdownMenu.Content>
      </DropdownMenu.Portal>
    </DropdownMenu.Root>
  );
}

export function ActionMenuItem({
  children,
  onSelect,
  destructive = false,
}: {
  children: React.ReactNode;
  onSelect: () => void;
  destructive?: boolean;
}) {
  return (
    <DropdownMenu.Item
      className={
        destructive
          ? "cursor-pointer rounded-xl px-3 py-2.5 text-sm font-medium text-rose-700 outline-none hover:bg-rose-50 focus:bg-rose-50"
          : "cursor-pointer rounded-xl px-3 py-2.5 text-sm font-medium text-slate-700 outline-none hover:bg-slate-100 focus:bg-slate-100"
      }
      onSelect={onSelect}
    >
      {children}
    </DropdownMenu.Item>
  );
}
