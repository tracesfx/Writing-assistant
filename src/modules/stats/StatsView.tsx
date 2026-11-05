/**
 * Goals and stats (SPEC §11.5): session, daily and project targets, a
 * deadline calculator, streak calendar, words per hour and active time.
 * Charts are plain CSS; Insights → Writing progress (M5) adds ECharts.
 */
import { useState } from "react";

import { story, useProject, useProjectStore, workbench, type Goals, type WorkspaceViewProps } from "@margin/sdk";

import { useStats } from "./store";

const fmt = (n: number) => n.toLocaleString("en-US");

function duration(s: number): string {
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  return h > 0 ? `${h} h ${m} min` : `${m} min`;
}

function isoDay(d: Date): string {
  const y = d.getFullYear();
  const m = String(d.getMonth() + 1).padStart(2, "0");
  const day = String(d.getDate()).padStart(2, "0");
  return `${y}-${m}-${day}`;
}

export function StatsStatusItem() {
  const s = useStats((x) => x.summary);
  if (!s) return null;
  const daily = s.goals.dailyWords;
  const today = Math.max(0, (s.today.words ?? 0));
  const pct = daily ? Math.min(100, Math.round((today / daily) * 100)) : null;
  return (
    <button
      className={`status-stats link${pct === 100 ? " done" : ""}`}
      title={`This session: ${fmt(s.sessionWords)} words${s.streakDays > 1 ? ` · ${s.streakDays}-day streak` : ""}. Click for stats and goals.`}
      onClick={() => workbench.openView("stats.overview")}
    >
      {daily ? `Today ${fmt(today)} / ${fmt(daily)}` : `Today ${fmt(today)} words`}
      {pct !== null && (
        <span className="mini-bar" aria-hidden="true">
          <span style={{ width: `${pct}%` }} />
        </span>
      )}
    </button>
  );
}

function Stat({ label, value, sub }: { label: string; value: string; sub?: string }) {
  return (
    <div className="stat">
      <div className="stat-label">{label}</div>
      <div className="stat-value">{value}</div>
      {sub && <div className="stat-sub">{sub}</div>}
    </div>
  );
}

export function StatsView(_props: WorkspaceViewProps) {
  const s = useStats((x) => x.summary);
  if (!s) return <p className="muted pad">Loading…</p>;
  const perHour = (s.today.activeSeconds ?? 0) >= 300 ? Math.round(Math.max(0, (s.today.words ?? 0)) / ((s.today.activeSeconds ?? 0) / 3600)) : null;
  const projectGoal = s.goals.projectWords;
  const projectPct = projectGoal ? Math.min(100, Math.round((s.projectWords / projectGoal) * 100)) : null;
  const byDay = new Map(s.days.map((d) => [d.date, d]));

  // Last 30 days, oldest first.
  const last30: { date: string; words: number }[] = [];
  for (let i = 29; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    const key = isoDay(d);
    last30.push({ date: key, words: Math.max(0, byDay.get(key)?.words ?? 0) });
  }
  const max = Math.max(1, ...last30.map((d) => d.words), s.goals.dailyWords ?? 0);

  // Streak calendar: 16 weeks ending this week, Monday first.
  const weeks: { date: string; words: number; future: boolean }[][] = [];
  const end = new Date();
  const start = new Date(end);
  start.setDate(end.getDate() - ((end.getDay() + 6) % 7) - 15 * 7);
  for (let w = 0; w < 16; w++) {
    const col = [];
    for (let d = 0; d < 7; d++) {
      const day = new Date(start);
      day.setDate(start.getDate() + w * 7 + d);
      const key = isoDay(day);
      col.push({ date: key, words: Math.max(0, byDay.get(key)?.words ?? 0), future: day > end });
    }
    weeks.push(col);
  }
  const level = (w: number) => (w <= 0 ? 0 : w < 250 ? 1 : w < 750 ? 2 : w < 1500 ? 3 : 4);

  return (
    <div className="stats-view">
      <h2>Writing stats</h2>
      <div className="stat-grid">
        <Stat label="Today" value={`${fmt((s.today.words ?? 0))} words`} {...(s.goals.dailyWords ? { sub: `Goal ${fmt(s.goals.dailyWords)}` } : {})} />
        <Stat label="This session" value={`${fmt(s.sessionWords)} words`} sub={duration(s.sessionActiveSeconds)} />
        <Stat label="Streak" value={`${s.streakDays} day${s.streakDays === 1 ? "" : "s"}`} />
        <Stat label="Time writing today" value={duration((s.today.activeSeconds ?? 0))} {...(perHour !== null ? { sub: `${fmt(perHour)} words per hour` } : {})} />
        <Stat label="Manuscript" value={`${fmt(s.projectWords)} words`} {...(projectGoal ? { sub: `${projectPct}% of ${fmt(projectGoal)}` } : {})} />
        {s.daysLeft !== null && (
          <Stat
            label="Deadline"
            value={s.daysLeft > 0 ? `${s.daysLeft} day${s.daysLeft === 1 ? "" : "s"} left` : "Passed"}
            {...(s.wordsPerDayNeeded !== null ? { sub: `${fmt(s.wordsPerDayNeeded)} words a day needed` } : {})}
          />
        )}
      </div>

      {projectPct !== null && (
        <div className="progress-line" aria-label={`Manuscript ${projectPct}% of goal`}>
          <span style={{ width: `${projectPct}%` }} />
        </div>
      )}

      <h3>Last 30 days</h3>
      <div className="bars" role="img" aria-label="Words written per day for the last 30 days">
        {s.goals.dailyWords ? <div className="bars-goal" style={{ bottom: `${(s.goals.dailyWords / max) * 100}%` }} title="Daily goal" /> : null}
        {last30.map((d) => (
          <div key={d.date} className="bar-col" title={`${d.date}: ${fmt(d.words)} words`}>
            <div className="bar-fill" style={{ height: `${(d.words / max) * 100}%` }} />
          </div>
        ))}
      </div>

      <h3>Streak calendar</h3>
      <div className="calendar" role="img" aria-label="Days with writing over the last 16 weeks">
        {weeks.map((col, i) => (
          <div key={i} className="cal-col">
            {col.map((d) => (
              <div key={d.date} className={`cal-day l${d.future ? "x" : level(d.words)}`} title={d.future ? "" : `${d.date}: ${fmt(d.words)} words`} />
            ))}
          </div>
        ))}
      </div>

      <h3>Goals</h3>
      <GoalsForm />
    </div>
  );
}

export function GoalsPage() {
  return (
    <section>
      <h2>Goals</h2>
      <GoalsForm />
    </section>
  );
}

type NumGoal = "projectWords" | "dailyWords" | "sessionWords";

function GoalsForm() {
  const project = useProject();
  const s = useStats((x) => x.summary);
  // Only the field being edited holds a draft; everything else shows the
  // stored goals, so a save from another field never clobbers typing.
  const [drafts, setDrafts] = useState<Partial<Record<NumGoal, string>>>({});
  const [now] = useState(() => Date.now());
  if (!project) return null;

  /** Merge one change into the latest stored goals (not a render-time copy). */
  const save = (patch: Partial<Goals>) => {
    const latest = useProjectStore.getState().info?.goals ?? project.goals;
    return story.project
      .update({ goals: { ...latest, ...patch } })
      .catch((e: unknown) => workbench.notify({ message: `Could not save goals: ${e instanceof Error ? e.message : String(e)}` }));
  };
  const parse = (v: string | undefined): number | null => (v ? Number(v) : null);
  const value = (k: NumGoal): number | null => (k in drafts ? parse(drafts[k]) : (project.goals[k] ?? null));

  const num = (k: NumGoal, label: string) => (
    <div className="settings-row">
      <div className="settings-label">
        <span>{label}</span>
      </div>
      <div className="settings-control">
        <input
          inputMode="numeric"
          aria-label={label}
          value={k in drafts ? (drafts[k] ?? "") : (project.goals[k]?.toString() ?? "")}
          placeholder="None"
          onChange={(e) => setDrafts((d) => ({ ...d, [k]: e.target.value.replace(/[^0-9]/g, "") }))}
          onBlur={() => {
            if (!(k in drafts)) return;
            const v = parse(drafts[k]);
            setDrafts(({ [k]: _done, ...rest }) => rest);
            if (v !== (project.goals[k] ?? null)) void save({ [k]: v });
          }}
        />
        <span className="muted">words</span>
      </div>
    </div>
  );

  // Deadline calculator preview, live as the author types.
  const projectGoal = value("projectWords");
  const deadline = project.goals.deadline;
  let preview = "";
  if (deadline && projectGoal && s) {
    const days = Math.floor((new Date(`${deadline}T23:59:59`).getTime() - now) / 86_400_000) + 1;
    if (days > 0) preview = `${fmt(Math.ceil(Math.max(0, projectGoal - s.projectWords) / days))} words a day for ${days} day${days === 1 ? "" : "s"}`;
    else preview = "The deadline has passed.";
  }

  return (
    <div className="goals-form">
      {num("projectWords", "Manuscript goal")}
      {num("dailyWords", "Daily goal")}
      {num("sessionWords", "Session goal")}
      <div className="settings-row">
        <div className="settings-label">
          <span>Deadline</span>
          {preview && <span className="settings-hint">{preview}</span>}
        </div>
        <div className="settings-control">
          <input type="date" aria-label="Deadline" value={deadline ?? ""} onChange={(e) => void save({ deadline: e.target.value || null })} />
        </div>
      </div>
    </div>
  );
}
