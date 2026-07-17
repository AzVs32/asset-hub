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
    <main className="grid min-h-screen place-items-center bg-slate-950 px-4 py-10">
      <section className="w-full max-w-sm rounded-3xl border border-white/10 bg-white p-8 shadow-2xl">
        <div className="mb-7 flex items-center gap-3">
          <span className="grid size-11 place-items-center rounded-2xl bg-blue-600 text-white">
            <Database size={22} />
          </span>
          <div>
            <h1 className="text-xl font-bold text-slate-950">Asset Hub</h1>
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
          {error ? <p className="rounded-lg bg-red-50 p-3 text-sm text-red-700">{error}</p> : null}
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
