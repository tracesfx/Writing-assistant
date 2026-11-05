import { workbench } from "@margin/sdk";

import { MODEL_VIEW } from "./ids";
import { STATUS_LABEL, useAi } from "./store";

/** The single quiet AI indicator (SPEC §2.2, §6.7). */
export function AiStatusItem() {
  const status = useAi((s) => s.status);
  const detail = useAi((s) => s.detail);
  return (
    <button
      className={`status-ai link ai-${status}`}
      title={`${detail || STATUS_LABEL[status]}. Click for the model panel.`}
      onClick={() => workbench.openView(MODEL_VIEW)}
    >
      <span className="ai-dot" aria-hidden="true" />
      AI: {STATUS_LABEL[status]}
    </button>
  );
}
