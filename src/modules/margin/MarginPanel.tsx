/**
 * The Margin panel (SPEC §8): collapsed by default, never opens itself. Its
 * tabs come from the `marginTabs` extension point.
 */
import { useMarginTabs, type SidebarPanelProps } from "@margin/sdk";

import { useMargin } from "./store";

export function MarginPanel({ visible }: SidebarPanelProps) {
  const tabs = useMarginTabs();
  const tab = useMargin((s) => s.tab);
  const fresh = useMargin((s) => s.fresh);
  const active = tabs.find((t) => t.id === tab) ?? tabs[0];
  return (
    <div className="margin-panel">
      <div className="margin-tabs" role="tablist">
        {tabs.map((t) => (
          <button
            key={t.id}
            role="tab"
            aria-selected={t.id === active?.id}
            className={t.id === active?.id ? "active" : ""}
            onClick={() => useMargin.setState({ tab: t.id, fresh: t.id === "margin.ideas" ? false : fresh })}
          >
            {t.title}
            {t.id === "margin.ideas" && fresh && t.id !== active?.id && <span className="margin-dot" aria-label="New ideas" />}
          </button>
        ))}
      </div>
      {tabs.map((t) => (
        <div key={t.id} className="margin-tab" role="tabpanel" hidden={t.id !== active?.id}>
          <t.component active={visible && t.id === active?.id} />
        </div>
      ))}
    </div>
  );
}
