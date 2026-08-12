import { cva, type VariantProps } from "class-variance-authority";
import type React from "react";
import { cn } from "./cn";

const buttonVariants = cva(
  "inline-flex min-h-10 items-center justify-center gap-2 rounded-xl px-4 text-sm font-semibold tracking-[-0.01em] transition-all duration-150 focus-visible:outline-none focus-visible:ring-4 active:translate-y-px disabled:pointer-events-none disabled:opacity-45 disabled:shadow-none",
  {
    variants: {
      variant: {
        primary:
          "bg-indigo-600 text-white shadow-[0_8px_20px_-10px_rgba(79,70,229,0.9)] hover:bg-indigo-500 hover:shadow-[0_10px_24px_-10px_rgba(79,70,229,0.95)] focus-visible:ring-indigo-200",
        secondary:
          "border border-slate-200/90 bg-white text-slate-700 shadow-sm hover:border-slate-300 hover:bg-slate-50 focus-visible:ring-slate-200",
        ghost:
          "text-slate-500 hover:bg-slate-100 hover:text-slate-900 focus-visible:ring-slate-200",
        danger:
          "bg-rose-600 text-white shadow-[0_8px_20px_-10px_rgba(225,29,72,0.9)] hover:bg-rose-500 focus-visible:ring-rose-200",
      },
      size: {
        default: "h-10",
        icon: "size-10 px-0",
        small: "min-h-9 px-3.5 text-xs",
      },
    },
    defaultVariants: { variant: "primary", size: "default" },
  },
);

export interface ButtonProps
  extends React.ButtonHTMLAttributes<HTMLButtonElement>,
    VariantProps<typeof buttonVariants> {}

export function Button({ className, variant, size, type = "button", ...props }: ButtonProps) {
  return (
    <button type={type} className={cn(buttonVariants({ variant, size }), className)} {...props} />
  );
}
