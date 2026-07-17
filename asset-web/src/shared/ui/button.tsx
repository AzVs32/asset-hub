import { cva, type VariantProps } from "class-variance-authority";
import type React from "react";
import { cn } from "./cn";

const buttonVariants = cva(
  "inline-flex min-h-10 items-center justify-center gap-2 rounded-lg px-4 text-sm font-semibold transition focus-visible:outline-none focus-visible:ring-4 disabled:pointer-events-none disabled:opacity-50",
  {
    variants: {
      variant: {
        primary: "bg-blue-600 text-white hover:bg-blue-700 focus-visible:ring-blue-200",
        secondary:
          "border border-slate-300 bg-white text-slate-700 hover:bg-slate-50 focus-visible:ring-slate-200",
        ghost: "text-slate-600 hover:bg-slate-100 focus-visible:ring-slate-200",
        danger: "bg-red-600 text-white hover:bg-red-700 focus-visible:ring-red-200",
      },
      size: {
        default: "h-10",
        icon: "size-10 px-0",
        small: "min-h-8 px-3 text-xs",
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
