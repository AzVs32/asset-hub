import { zodResolver } from "@hookform/resolvers/zod";
import { Database, LoaderCircle } from "lucide-react";
import { useForm } from "react-hook-form";
import { z } from "zod";
import { Button } from "@/shared/ui/button";
import { Field, Input } from "@/shared/ui/field";

const loginSchema = z.object({
  username: z.string().trim().min(1, "Username is required"),
  password: z.string().min(1, "Password is required"),
});

type LoginInput = z.infer<typeof loginSchema>;

export function LoginForm({
  onSubmit,
  error,
}: {
  onSubmit: (input: LoginInput) => Promise<void>;
  error: string | null;
}) {
  const form = useForm<LoginInput>({
    resolver: zodResolver(loginSchema),
    defaultValues: { username: "", password: "" },
  });

  return (
    <main className="relative grid min-h-screen place-items-center overflow-hidden bg-slate-950 px-4 py-10">
      <div className="pointer-events-none absolute -left-32 -top-32 size-[28rem] rounded-full bg-indigo-500/20 blur-3xl" />
      <div className="pointer-events-none absolute -bottom-40 -right-24 size-[32rem] rounded-full bg-blue-500/15 blur-3xl" />
      <section className="relative w-full max-w-sm rounded-[2rem] border border-white/70 bg-white/95 p-8 shadow-[0_35px_100px_-35px_rgba(0,0,0,0.8)] backdrop-blur-xl">
        <div className="mb-7 flex items-center gap-3">
          <span className="grid size-11 place-items-center rounded-2xl bg-gradient-to-br from-indigo-500 to-blue-500 text-white shadow-[0_12px_28px_-12px_rgba(79,70,229,0.9)]">
            <Database size={22} />
          </span>
          <div>
            <h1 className="text-xl font-bold tracking-[-0.03em] text-slate-950">Asset Hub</h1>
            <p className="text-sm text-slate-500">Sign in to your workspace</p>
          </div>
        </div>
        <form className="grid gap-4" onSubmit={form.handleSubmit(onSubmit)}>
          <Field label="Username" error={form.formState.errors.username?.message}>
            <Input autoComplete="username" {...form.register("username")} />
          </Field>
          <Field label="Password" error={form.formState.errors.password?.message}>
            <Input type="password" autoComplete="current-password" {...form.register("password")} />
          </Field>
          {error ? (
            <p className="rounded-xl border border-rose-200 bg-rose-50 p-3 text-sm text-rose-700">
              {error}
            </p>
          ) : null}
          <Button className="mt-2" type="submit" disabled={form.formState.isSubmitting}>
            {form.formState.isSubmitting ? (
              <LoaderCircle className="animate-spin" size={18} />
            ) : null}
            Sign in
          </Button>
        </form>
      </section>
    </main>
  );
}
