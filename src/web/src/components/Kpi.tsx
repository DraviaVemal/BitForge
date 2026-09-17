import type { ReactNode } from "react";

type KpiTone = "default" | "accent" | "amber" | "emerald" | "cyan";

export function KpiGrid({ children }: { children: ReactNode }) {
  return <div className="kpi-grid">{children}</div>;
}

export function Kpi({
  label,
  value,
  tone = "default",
  hint,
  hintTone,
}: {
  label: string;
  value: ReactNode;
  tone?: KpiTone;
  hint?: ReactNode;
  hintTone?: "default" | "emerald" | "amber";
}) {
  return (
    <div className="kpi">
      <span className="kpi-label">{label}</span>
      <span className={`kpi-value ${tone}`}>{value}</span>
      {hint !== undefined && <span className={`kpi-hint ${hintTone ?? "default"}`}>{hint}</span>}
    </div>
  );
}
