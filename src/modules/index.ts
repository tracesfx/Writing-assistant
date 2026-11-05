/** First-party modules, compiled in (SPEC §3.2: no third-party loading in v1). */
import type { ModuleManifest } from "@margin/sdk";

import binder from "./binder/module";
import comments from "./comments/module";
import editor from "./editor/module";
import history from "./history/module";
import inspector from "./inspector/module";
import notes from "./notes/module";
import search from "./search/module";
import stats from "./stats/module";

export const firstPartyModules: ModuleManifest[] = [editor, binder, search, inspector, comments, notes, history, stats];
