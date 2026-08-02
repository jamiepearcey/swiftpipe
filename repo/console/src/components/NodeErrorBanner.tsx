// Honest failure banner. Distinguishes an auth problem (401/403 from the shared
// auth bus) from a generic fetch failure and points at the remediation. Renders
// nothing when everything is fine.
import { cn } from "@/lib/utils";
import { AlertTriangle, Lock } from "lucide-react";
import { authMessage, useAuthState } from "@/lib/auth";

export function NodeErrorBanner({ error, className }: { error?: string | null; className?: string }) {
  const auth = useAuthState();

  if (auth.status !== 0) {
    return (
      <div className={cn("flex items-start gap-2 rounded-lg border border-warn/45 bg-warn/10 px-3 py-2.5 text-[12px] text-warn", className)}>
        <Lock className="mt-0.5 h-4 w-4 shrink-0" />
        <div>
          <div className="font-medium">{auth.status === 401 ? "Authentication required" : "Not authorized"}</div>
          <div className="text-warn/85">{authMessage(auth)}</div>
        </div>
      </div>
    );
  }

  if (error) {
    return (
      <div className={cn("flex items-center gap-2 rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-[12px] text-destructive", className)}>
        <AlertTriangle className="h-4 w-4 shrink-0" /> {error}
      </div>
    );
  }

  return null;
}
