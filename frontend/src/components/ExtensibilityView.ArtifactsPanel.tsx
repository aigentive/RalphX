/**
 * ArtifactsPanel - Browse artifacts by bucket with grid/list toggle
 */

import { useState } from "react";
import {
  FileBox,
  Search,
  List,
  LayoutGrid,
  ArrowUpDown,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Input } from "@/components/ui/input";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { AGENT_ORCHESTRATOR, AGENT_DEEP_RESEARCHER } from "@/constants/agents";
import type { Artifact, ArtifactBucket } from "@/types/artifact";
import { getFileIcon, type ViewMode, type SortBy } from "./ExtensibilityView.utils";

export function ArtifactsPanel() {
  const [viewMode, setViewMode] = useState<ViewMode>("grid");
  const [selectedBucket, setSelectedBucket] = useState<string | null>(null);
  const [_sortBy, setSortBy] = useState<SortBy>("name");
  const [searchQuery, setSearchQuery] = useState("");

  // Mock data - would come from API
  const buckets: ArtifactBucket[] = [
    { id: "all", name: "All", acceptedTypes: [], writers: [], readers: ["all"], isSystem: false },
    { id: "system", name: "System", acceptedTypes: ["context", "activity_log"], writers: ["system"], readers: ["all"], isSystem: true },
    { id: "prds", name: "PRDs", acceptedTypes: ["prd", "specification"], writers: [AGENT_ORCHESTRATOR, "user"], readers: ["all"], isSystem: false },
    { id: "docs", name: "Docs", acceptedTypes: ["research_document", "design_doc"], writers: ["user"], readers: ["all"], isSystem: false },
  ];

  const artifacts: Artifact[] = [
    {
      id: "1",
      name: "PRD.md",
      type: "prd",
      content: { type: "file", path: "/docs/PRD.md" },
      metadata: { createdAt: "2026-01-01T00:00:00Z", createdBy: "user", version: 1 },
      derivedFrom: [],
      bucketId: "prds",
    },
    {
      id: "2",
      name: "Research Notes",
      type: "research_document",
      content: { type: "inline", text: "Research content here..." },
      metadata: { createdAt: "2026-01-01T00:00:00Z", createdBy: AGENT_DEEP_RESEARCHER, version: 1 },
      derivedFrom: [],
      bucketId: "docs",
    },
  ];

  const filteredArtifacts = artifacts.filter((a) => {
    if (selectedBucket && selectedBucket !== "all" && a.bucketId !== selectedBucket) {
      return false;
    }
    if (searchQuery && !a.name.toLowerCase().includes(searchQuery.toLowerCase())) {
      return false;
    }
    return true;
  });

  const bucketCounts = artifacts.reduce<Record<string, number>>((acc, a) => {
    if (a.bucketId) {
      acc[a.bucketId] = (acc[a.bucketId] ?? 0) + 1;
    }
    acc["all"] = (acc["all"] ?? 0) + 1;
    return acc;
  }, {});

  return (
    <div data-testid="artifacts-panel" className="flex h-full gap-4">
      {/* Bucket Sidebar */}
      <div
        className="w-48 flex-shrink-0 p-3 rounded-lg"
        style={{
          background: "var(--overlay-faint)",
          backdropFilter: "blur(20px)",
          WebkitBackdropFilter: "blur(20px)",
          border: "1px solid var(--overlay-weak)",
        }}
      >
        <h3
          className="text-xs font-medium uppercase tracking-wide mb-3"
          style={{ color: "var(--text-muted)" }}
        >
          Buckets
        </h3>
        <div className="space-y-1">
          {buckets.map((bucket) => {
            const isSelected = selectedBucket === bucket.id;
            const count = bucketCounts[bucket.id] ?? 0;
            return (
              <button
                key={bucket.id}
                data-testid="bucket-item"
                onClick={() => setSelectedBucket(bucket.id)}
                className="w-full flex items-center justify-between px-2 py-1.5 rounded text-sm transition-colors hover:bg-[--bg-hover]"
                style={{
                  backgroundColor: isSelected ? "var(--bg-hover)" : undefined,
                  color: isSelected
                    ? "var(--text-primary)"
                    : "var(--text-secondary)",
                }}
              >
                <div className="flex items-center gap-1.5">
                  <span className="truncate">{bucket.name}</span>
                  {bucket.isSystem && (
                    <Badge
                      variant="secondary"
                      className="text-[0.625rem] px-1 py-0"
                    >
                      S
                    </Badge>
                  )}
                </div>
                <span className="text-xs" style={{ color: "var(--text-muted)" }}>
                  {count}
                </span>
              </button>
            );
          })}
        </div>
      </div>

      {/* Artifact Content */}
      <div className="flex-1 space-y-4">
        {/* Search & Filter Bar */}
        <div className="flex items-center gap-3">
          <div className="relative flex-1">
            <Search
              className="absolute left-3 top-1/2 -translate-y-1/2 w-4 h-4"
              style={{ color: "var(--text-muted)" }}
            />
            <Input
              placeholder="Search artifacts..."
              value={searchQuery}
              onChange={(e) => setSearchQuery(e.target.value)}
              className="pl-9"
            />
          </div>
          <div className="flex items-center gap-1 p-1 rounded-md" style={{ backgroundColor: "var(--bg-surface)" }}>
            <Button
              variant={viewMode === "list" ? "secondary" : "ghost"}
              size="sm"
              className="h-7 w-7 p-0"
              onClick={() => setViewMode("list")}
            >
              <List className="w-4 h-4" />
            </Button>
            <Button
              variant={viewMode === "grid" ? "secondary" : "ghost"}
              size="sm"
              className="h-7 w-7 p-0"
              onClick={() => setViewMode("grid")}
            >
              <LayoutGrid className="w-4 h-4" />
            </Button>
          </div>
          <Select defaultValue="name" onValueChange={(v) => setSortBy(v as SortBy)}>
            <SelectTrigger className="w-auto gap-1.5">
              <ArrowUpDown className="w-4 h-4" />
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="name">Name</SelectItem>
              <SelectItem value="date">Date</SelectItem>
              <SelectItem value="size">Size</SelectItem>
              <SelectItem value="type">Type</SelectItem>
            </SelectContent>
          </Select>
        </div>

        {/* Artifact Display */}
        {!selectedBucket ? (
          <div
            className="flex flex-col items-center justify-center py-16"
            style={{ color: "var(--text-muted)" }}
          >
            <FileBox className="w-12 h-12 mb-3" />
            <p className="text-sm">Select a bucket to view artifacts</p>
          </div>
        ) : filteredArtifacts.length === 0 ? (
          <div
            className="flex flex-col items-center justify-center py-16"
            style={{ color: "var(--text-muted)" }}
          >
            <div
              className="w-16 h-16 rounded-xl flex items-center justify-center mb-4"
              style={{ border: "2px dashed var(--border-subtle)" }}
            >
              <FileBox className="w-8 h-8" />
            </div>
            <p className="text-sm">No artifacts in this bucket</p>
          </div>
        ) : viewMode === "grid" ? (
          <div className="grid grid-cols-4 gap-3">
            {filteredArtifacts.map((artifact) => {
              const IconComponent = getFileIcon(artifact.type);
              return (
                <Card
                  key={artifact.id}
                  data-testid="artifact-card"
                  className="group cursor-pointer transition-all duration-180 hover:-translate-y-px"
                  style={{
                    background: "var(--overlay-faint)",
                    backdropFilter: "blur(20px)",
                    WebkitBackdropFilter: "blur(20px)",
                    border: "1px solid var(--border-subtle)",
                    boxShadow: "var(--shadow-xs)",
                  }}
                >
                  <CardContent className="p-3 text-center">
                    <IconComponent
                      className="w-8 h-8 mx-auto mb-2"
                      style={{ color: "var(--text-secondary)" }}
                    />
                    <p
                      className="text-sm truncate"
                      style={{ color: "var(--text-primary)" }}
                    >
                      {artifact.name}
                    </p>
                    <p className="text-xs" style={{ color: "var(--text-muted)" }}>
                      {artifact.type}
                    </p>
                  </CardContent>
                </Card>
              );
            })}
          </div>
        ) : (
          <div className="space-y-1">
            {filteredArtifacts.map((artifact) => {
              const IconComponent = getFileIcon(artifact.type);
              return (
                <div
                  key={artifact.id}
                  data-testid="artifact-row"
                  className="flex items-center gap-3 px-3 py-2 rounded-md cursor-pointer transition-colors hover:bg-[--bg-hover]"
                >
                  <IconComponent
                    className="w-5 h-5"
                    style={{ color: "var(--text-secondary)" }}
                  />
                  <span
                    className="flex-1 text-sm"
                    style={{ color: "var(--text-primary)" }}
                  >
                    {artifact.name}
                  </span>
                  <Badge variant="secondary" className="text-[0.625rem]">
                    {artifact.type}
                  </Badge>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </div>
  );
}
