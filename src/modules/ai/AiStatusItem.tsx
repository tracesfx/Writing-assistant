import { CONTEXT_VIEW, ai, workbench } from "@margin/sdk";

import { MODEL_VIEW } from "./ids";
import { STATUS_LABEL, useAi } from "./store";

/** The single quiet AI indicator (SPEC §2.2, §6.7). */
export function AiStatusItem() {
  const status = useAi((s) => s.status);
  const detail = useAi((s) => s.detail);
  return (
    <button
      className={`status-ai link ai-${status}`}
      title={`${detail || STATUS_LABEL[status]}. Click for the model panel; right-click to see what was last sent to the model.`}
      onClick={() => workbench.openView(MODEL_VIEW)}
      onContextMenu={(e) => {
        e.preventDefault();
        void ai.contextLog.list(1).then((l) => (l[0] ? workbench.openView(CONTEXT_VIEW, l[0].id) : workbench.openView(MODEL_VIEW)));
      }}
    >
      <span className="ai-dot" aria-hidden="true" />
      AI: {STATUS_LABEL[status]}
    </button>
  );
}
