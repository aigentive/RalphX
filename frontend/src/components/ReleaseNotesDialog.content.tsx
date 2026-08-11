import { memo, Suspense } from "react";
import { Loader2 } from "lucide-react";

import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { formatDay } from "./ReleaseNotesDialog.sidebar-items";

const LazyMarkdown = lazyWithRetry(async () => {
  const [{ default: ReactMarkdown }, { default: remarkGfm }] =
    await Promise.all([import("react-markdown"), import("remark-gfm")]);

  return {
    default: memo(function ReleaseMarkdown({ body }: { body: string }) {
      return (
        <ReactMarkdown
          remarkPlugins={[remarkGfm]}
          components={markdownComponents}
          skipHtml
        >
          {body}
        </ReactMarkdown>
      );
    }),
  };
});
export const VersionContent = memo(function VersionContent({
  version,
  body,
  loading,
  date,
}: {
  version: string;
  body: string | null | undefined;
  loading: boolean;
  date: string | null;
}) {
  return (
    <div className="px-8 py-6">
      <div className="mb-4 flex items-baseline gap-3">
        <h2
          className="text-lg font-semibold"
          style={{ color: "var(--text-primary)" }}
        >
          v{version}
        </h2>
        {date && (
          <span
            className="text-[0.75rem]"
            style={{ color: "var(--text-muted)" }}
          >
            {formatDay(date)}
          </span>
        )}
      </div>

      {loading || body === undefined ? (
        <div className="flex items-center gap-2 py-8">
          <Loader2
            className="h-4 w-4 animate-spin"
            style={{ color: "var(--text-muted)" }}
          />
          <span className="text-sm" style={{ color: "var(--text-muted)" }}>
            Loading...
          </span>
        </div>
      ) : body ? (
        <div
          className="text-[0.8125rem] leading-relaxed"
          style={{ color: "var(--text-primary)" }}
        >
          <Suspense
            fallback={
              <pre className="whitespace-pre-wrap font-sans">{body}</pre>
            }
          >
            <LazyMarkdown body={body} />
          </Suspense>
        </div>
      ) : (
        <p className="py-4 text-sm" style={{ color: "var(--text-muted)" }}>
          Release notes not available for this version.
        </p>
      )}
    </div>
  );
});
