import type { LucideIcon } from "lucide-react";
import { cn } from "@/lib/utils";
import { Btn } from "@/components/setup/kit";

export function PillarEmpty({
  icon: Icon,
  title,
  body,
  cta,
  onCta,
  toneClass = "text-muted-foreground",
  iconWrapClass = "border-outline-subtle bg-surface-app text-icon-muted",
}: {
  icon: LucideIcon;
  title: string;
  body: string;
  cta?: string;
  onCta?: () => void;
  toneClass?: string;
  iconWrapClass?: string;
}) {
  return (
    <div className="flex flex-col items-center rounded-xl border border-dashed border-outline-strong px-6 py-12 text-center">
      <div className={cn("grid size-12 place-items-center rounded-xl border", iconWrapClass)}>
        <Icon className={cn("size-5", toneClass)} />
      </div>
      <p className="mt-4 text-[14px] font-semibold text-foreground">{title}</p>
      <p className="mt-1.5 max-w-sm text-[12.5px] leading-snug text-muted-foreground">{body}</p>
      {cta && onCta && (
        <Btn variant="primary" onClick={onCta} className="mt-5 inline-flex items-center gap-1.5">
          {cta}
        </Btn>
      )}
    </div>
  );
}
