/**
 * WorkflowsPanel - Workflow management with cards
 */

import {
  Workflow,
  Plus,
  Edit,
  Copy,
  Trash2,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { WorkflowSchema } from "@/types/workflow";

export function WorkflowsPanel() {
  // Mock data for now - would come from API
  const workflows: WorkflowSchema[] = [
    {
      id: "default",
      name: "Default Kanban",
      description: "Standard development workflow",
      columns: [
        { id: "1", name: "Backlog", mapsTo: "backlog" },
        { id: "2", name: "In Progress", mapsTo: "executing" },
        { id: "3", name: "Review", mapsTo: "pending_review" },
        { id: "4", name: "Done", mapsTo: "approved" },
      ],
      isDefault: true,
    },
  ];

  const isEmpty = workflows.length === 0;

  return (
    <div data-testid="workflows-panel" className="space-y-4">
      {/* Header */}
      <div className="flex items-center justify-between">
        <h2
          className="text-lg font-semibold"
          style={{
            color: "var(--text-primary)",
            letterSpacing: "-0.02em",
          }}
        >
          Workflow Schemas
        </h2>
        <Button variant="secondary" size="sm" className="gap-1.5">
          <Plus className="w-4 h-4" />
          New Workflow
        </Button>
      </div>

      {/* Empty State */}
      {isEmpty ? (
        <div className="flex flex-col items-center justify-center py-16">
          <div
            className="w-16 h-16 rounded-xl flex items-center justify-center mb-4"
            style={{
              border: "2px dashed var(--border-subtle)",
            }}
          >
            <Workflow
              className="w-8 h-8"
              style={{ color: "var(--text-muted)" }}
            />
          </div>
          <p
            className="text-sm font-medium mb-1"
            style={{ color: "var(--text-secondary)" }}
          >
            No custom workflows yet
          </p>
          <p className="text-xs mb-4" style={{ color: "var(--text-muted)" }}>
            Create a workflow to organize tasks
          </p>
          <Button className="gap-1.5">
            <Plus className="w-4 h-4" />
            Create Workflow
          </Button>
        </div>
      ) : (
        /* Workflow Cards */
        <div className="space-y-3">
          {workflows.map((workflow) => (
            <Card
              key={workflow.id}
              data-testid="workflow-card"
              className="group transition-all duration-180 hover:-translate-y-px"
              style={{
                background: "var(--overlay-faint)",
                backdropFilter: "blur(20px)",
                WebkitBackdropFilter: "blur(20px)",
                border: "1px solid var(--border-subtle)",
                boxShadow: "var(--shadow-xs)",
              }}
            >
              <CardContent className="p-4">
                {/* Header Row */}
                <div className="flex items-center justify-between mb-2">
                  <div className="flex items-center gap-2">
                    {workflow.isDefault && (
                      <div
                        className="w-2 h-2 rounded-full"
                        style={{ backgroundColor: "var(--accent-primary)" }}
                      />
                    )}
                    <span
                      className="text-sm font-medium"
                      style={{ color: "var(--text-primary)" }}
                    >
                      {workflow.name}
                    </span>
                    {workflow.isDefault && (
                      <Badge
                        variant="secondary"
                        className="text-[0.625rem]"
                        style={{
                          backgroundColor: "var(--bg-hover)",
                          color: "var(--text-secondary)",
                          border: "1px solid var(--border-subtle)",
                        }}
                      >
                        DEFAULT
                      </Badge>
                    )}
                  </div>
                  <div className="flex gap-1 opacity-0 group-hover:opacity-100 transition-opacity">
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7 w-7 p-0"
                        >
                          <Edit className="w-4 h-4" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>Edit</TooltipContent>
                    </Tooltip>
                    <Tooltip>
                      <TooltipTrigger asChild>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="h-7 w-7 p-0"
                        >
                          <Copy className="w-4 h-4" />
                        </Button>
                      </TooltipTrigger>
                      <TooltipContent>Duplicate</TooltipContent>
                    </Tooltip>
                    {!workflow.isDefault && (
                      <Tooltip>
                        <TooltipTrigger asChild>
                          <Button
                            variant="ghost"
                            size="sm"
                            className="h-7 w-7 p-0 text-status-error hover:text-status-error"
                          >
                            <Trash2 className="w-4 h-4" />
                          </Button>
                        </TooltipTrigger>
                        <TooltipContent>Delete</TooltipContent>
                      </Tooltip>
                    )}
                  </div>
                </div>

                {/* Description */}
                <p
                  className="text-sm line-clamp-2 mb-2"
                  style={{ color: "var(--text-secondary)" }}
                >
                  {workflow.description}
                </p>

                {/* Metadata */}
                <div
                  className="text-xs flex items-center gap-2"
                  style={{ color: "var(--text-muted)" }}
                >
                  <span>{workflow.columns.length} columns</span>
                  <span>·</span>
                  <span>Created Jan 2026</span>
                </div>
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}
