import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { RefreshCw } from "lucide-react";
import { toast } from "sonner";

import {
  projectSkillsApi,
  type ConversationProjectSkill,
} from "@/api/project-skills";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";

interface ConversationSkillsPanelProps {
  projectId: string;
  conversationId: string;
}

const conversationSkillsKey = (projectId: string, conversationId: string) =>
  ["project-skills", projectId, "conversation", conversationId] as const;

export function ConversationSkillsPanel({
  projectId,
  conversationId,
}: ConversationSkillsPanelProps) {
  const queryClient = useQueryClient();
  const queryKey = conversationSkillsKey(projectId, conversationId);
  const query = useQuery({
    queryKey,
    queryFn: () => projectSkillsApi.listForConversation({ projectId, conversationId }),
  });
  const processMutation = useMutation({
    mutationFn: () => projectSkillsApi.processConversation({ projectId, conversationId }),
    onSuccess: (result) => {
      queryClient.invalidateQueries({ queryKey });
      const staged = result.stagedSkills.length;
      if (staged > 0) {
        toast.success(
          `Staged ${staged} skill${staged === 1 ? "" : "s"} from this conversation.`,
        );
      } else {
        toast.info("Conversation skill processing is up to date.");
      }
    },
    onError: () => {
      toast.error("Failed to process conversation skills.");
    },
  });
  const isRefreshing = query.isFetching || processMutation.isPending;

  return (
    <div
      className="flex h-full min-h-0 flex-col"
      style={{ backgroundColor: "var(--bg-base)" }}
      data-testid="conversation-skills-panel"
    >
      <div className="flex items-center justify-between gap-3 border-b px-5 py-3"
        style={{ borderBottomColor: "var(--border-subtle)" }}
      >
        <div className="min-w-0">
          <h2
            className="text-[0.875rem] font-semibold"
            style={{ color: "var(--text-primary)", letterSpacing: "0" }}
          >
            Skills
          </h2>
          <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
            Generated or used in this conversation
          </p>
        </div>
        <Button
          type="button"
          size="sm"
          variant="outline"
          onClick={() => processMutation.mutate()}
          disabled={isRefreshing}
          aria-label="Process conversation skills"
        >
          <RefreshCw className={isRefreshing ? "animate-spin" : undefined} />
        </Button>
      </div>

      <div className="min-h-0 flex-1 overflow-auto px-5 py-4">
        {query.isLoading ? (
          <ConversationSkillsSkeleton />
        ) : query.isError ? (
          <ConversationSkillsState message="Failed to load conversation skills." />
        ) : query.data && query.data.length > 0 ? (
          <div className="mx-auto flex w-full max-w-[820px] flex-col gap-3">
            {query.data.map((row) => (
              <ConversationSkillCard key={row.skill.id} row={row} />
            ))}
          </div>
        ) : (
          <ConversationSkillsState message="No skills generated or used in this conversation." />
        )}
      </div>
    </div>
  );
}

function ConversationSkillCard({ row }: { row: ConversationProjectSkill }) {
  const { skill } = row;

  return (
    <Card className="rounded-[8px] border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4">
      <div className="flex items-start justify-between gap-3">
        <div className="min-w-0 flex-1">
          <div className="flex flex-wrap items-center gap-2">
            <h3
              className="text-[0.875rem] font-semibold"
              style={{ color: "var(--text-primary)", letterSpacing: "0" }}
            >
              {skill.title}
            </h3>
            <Badge variant="outline">{skill.status}</Badge>
            {row.generatedByConversation && (
              <Badge variant="secondary">Generated here</Badge>
            )}
            {row.usedByConversation && (
              <Badge variant="secondary">
                Used {row.usageCount} {row.usageCount === 1 ? "time" : "times"}
              </Badge>
            )}
          </div>
          <p className="mt-2 text-[0.8125rem]" style={{ color: "var(--text-secondary)" }}>
            {skill.compactGuidance}
          </p>
          {skill.predictedEffect && (
            <p className="mt-2 text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
              {skill.predictedEffect}
            </p>
          )}
        </div>
        <div className="shrink-0 text-right text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
          <div>{skill.bucket}</div>
          <div>{skill.stage}</div>
        </div>
      </div>
    </Card>
  );
}

function ConversationSkillsSkeleton() {
  return (
    <div className="mx-auto flex w-full max-w-[820px] flex-col gap-3">
      {[0, 1, 2].map((index) => (
        <Card
          key={index}
          className="rounded-[8px] border-[var(--border-subtle)] bg-[var(--bg-elevated)] p-4"
        >
          <Skeleton className="h-4 w-56" />
          <Skeleton className="mt-3 h-3 w-full" />
          <Skeleton className="mt-2 h-3 w-2/3" />
        </Card>
      ))}
    </div>
  );
}

function ConversationSkillsState({ message }: { message: string }) {
  return (
    <div
      className="flex h-full items-center justify-center text-center text-[0.8125rem]"
      style={{ color: "var(--text-muted)" }}
    >
      {message}
    </div>
  );
}
