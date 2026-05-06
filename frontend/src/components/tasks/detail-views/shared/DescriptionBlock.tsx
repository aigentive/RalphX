/**
 * DescriptionBlock - Clean description text with empty state
 */
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";

interface DescriptionBlockProps {
  description: string | null | undefined;
  testId?: string;
}

export function DescriptionBlock({ description, testId }: DescriptionBlockProps) {
  if (!description) {
    return (
      <p className="text-[0.8125rem] italic text-text-primary/30">
        No description provided
      </p>
    );
  }

  return (
    <div
      data-testid={testId}
      className="text-[0.8125rem] text-text-primary/65 leading-relaxed"
      style={{ wordBreak: "break-word" }}
    >
      <ReactMarkdown remarkPlugins={[remarkGfm]} components={markdownComponents}>
        {description}
      </ReactMarkdown>
    </div>
  );
}
