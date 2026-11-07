import { InsightHeader, InsightPanel, story, useInsightsData, type InsightsViewProps } from "@margin/sdk";

/** The whole view: ask the Story Data Service for something, draw it. */
export function HelloView({ active }: InsightsViewProps) {
  // `useInsightsData` loads when the view is showing and again whenever the
  // project, its memory or its stats change. Nothing else to wire up.
  const { data, error } = useInsightsData(() => story.stats.summary(), active);

  return (
    <div className="insight">
      <InsightHeader title="Hello view" intro="A sample Insights view: one folder, one manifest." />
      <InsightPanel>
        {error && <p role="alert">{error}</p>}
        {data && (
          <p>
            Your manuscript has <strong>{data.projectWords.toLocaleString("en-US")}</strong> words, and you have written{" "}
            <strong>{(data.today.words ?? 0).toLocaleString("en-US")}</strong> today.
          </p>
        )}
      </InsightPanel>
    </div>
  );
}
