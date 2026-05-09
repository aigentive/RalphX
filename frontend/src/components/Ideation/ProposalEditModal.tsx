/**
 * ProposalEditModal - Modal for editing task proposal details
 * Clean single-column layout with proper scrolling
 */

import { useState, useCallback, useEffect, useRef } from "react";
import { Edit3, Plus, X, Loader2 } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
} from "@/components/ui/dialog";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { Label } from "@/components/ui/label";
import type { TaskProposal, Priority, Complexity } from "@/types/ideation";
import type { UpdateProposalInput } from "@/api/ideation";

const CATEGORIES = [
  { value: "setup", label: "Setup" },
  { value: "feature", label: "Feature" },
  { value: "integration", label: "Integration" },
  { value: "styling", label: "Styling" },
  { value: "testing", label: "Testing" },
  { value: "documentation", label: "Documentation" },
];

const PRIORITIES: { value: Priority | ""; label: string }[] = [
  { value: "", label: "Auto" },
  { value: "critical", label: "Critical" },
  { value: "high", label: "High" },
  { value: "medium", label: "Medium" },
  { value: "low", label: "Low" },
];

const COMPLEXITIES: { value: Complexity; label: string }[] = [
  { value: "trivial", label: "Trivial" },
  { value: "simple", label: "Simple" },
  { value: "moderate", label: "Moderate" },
  { value: "complex", label: "Complex" },
  { value: "very_complex", label: "Very Complex" },
];

interface ComplexitySelectorProps {
  value: Complexity;
  onChange: (value: Complexity) => void;
  disabled?: boolean;
}

function ComplexitySelector({ value, onChange, disabled }: ComplexitySelectorProps) {
  const selectedIndex = COMPLEXITIES.findIndex((c) => c.value === value);

  return (
    <div className="flex items-center gap-3">
      <div className="flex items-center gap-1.5">
        {COMPLEXITIES.map((c, index) => {
          const isSelected = index <= selectedIndex;
          return (
            <button
              key={c.value}
              type="button"
              onClick={() => !disabled && onChange(c.value)}
              disabled={disabled}
              title={c.label}
              aria-label={`Set complexity to ${c.label}`}
              className={`
                w-3 h-3 rounded-full transition-colors duration-150
                ${isSelected
                  ? "bg-[var(--accent-primary)]"
                  : "bg-[var(--overlay-moderate)] hover:bg-[var(--accent-muted)]"
                }
                ${!disabled ? "cursor-pointer" : "cursor-not-allowed opacity-50"}
              `}
            />
          );
        })}
      </div>
      <span className="text-sm text-[var(--text-secondary)]">
        {COMPLEXITIES[selectedIndex]?.label ?? "Moderate"}
      </span>
    </div>
  );
}

interface ProposalEditModalProps {
  proposal: TaskProposal | null;
  onSave: (proposalId: string, data: UpdateProposalInput) => void;
  onCancel: () => void;
  isSaving?: boolean;
}

export function ProposalEditModal({
  proposal,
  onSave,
  onCancel,
  isSaving = false,
}: ProposalEditModalProps) {
  const [title, setTitle] = useState("");
  const [description, setDescription] = useState("");
  const [category, setCategory] = useState("");
  const [steps, setSteps] = useState<string[]>([]);
  const [acceptanceCriteria, setAcceptanceCriteria] = useState<string[]>([]);
  const [userPriority, setUserPriority] = useState<Priority | "">("");
  const [complexity, setComplexity] = useState<Complexity>("moderate");

  const titleInputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (proposal) {
      setTitle(proposal.title);
      setDescription(proposal.description ?? "");
      setCategory(proposal.category);
      setSteps([...proposal.steps]);
      setAcceptanceCriteria([...proposal.acceptanceCriteria]);
      setUserPriority(proposal.userPriority ?? "");
      setComplexity(proposal.estimatedComplexity);
    }
  }, [proposal]);

  useEffect(() => {
    if (proposal && titleInputRef.current) {
      titleInputRef.current.focus();
    }
  }, [proposal]);

  const handleOpenChange = useCallback(
    (open: boolean) => {
      if (!open && !isSaving) {
        onCancel();
      }
    },
    [onCancel, isSaving]
  );

  const handleStepChange = useCallback((index: number, value: string) => {
    setSteps((prev) => prev.map((s, i) => (i === index ? value : s)));
  }, []);

  const handleAddStep = useCallback(() => {
    setSteps((prev) => [...prev, ""]);
  }, []);

  const handleRemoveStep = useCallback((index: number) => {
    setSteps((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleCriterionChange = useCallback((index: number, value: string) => {
    setAcceptanceCriteria((prev) => prev.map((c, i) => (i === index ? value : c)));
  }, []);

  const handleAddCriterion = useCallback(() => {
    setAcceptanceCriteria((prev) => [...prev, ""]);
  }, []);

  const handleRemoveCriterion = useCallback((index: number) => {
    setAcceptanceCriteria((prev) => prev.filter((_, i) => i !== index));
  }, []);

  const handleSave = useCallback(() => {
    if (!proposal || !title.trim()) return;

    const trimmedDescription = description.trim();
    const baseData = {
      title: title.trim(),
      category,
      steps: steps.filter((s) => s.trim() !== ""),
      acceptanceCriteria: acceptanceCriteria.filter((c) => c.trim() !== ""),
      complexity,
    };

    // Build final data - conditionally add optional fields to avoid undefined values
    // with exactOptionalPropertyTypes
    const data: UpdateProposalInput = trimmedDescription && userPriority
      ? { ...baseData, description: trimmedDescription, userPriority }
      : trimmedDescription
        ? { ...baseData, description: trimmedDescription }
        : userPriority
          ? { ...baseData, userPriority }
          : baseData;

    onSave(proposal.id, data);
  }, [
    proposal,
    title,
    description,
    category,
    steps,
    acceptanceCriteria,
    userPriority,
    complexity,
    onSave,
  ]);

  if (!proposal) return null;

  const canSave = title.trim().length > 0 && !isSaving;

  const inputClasses = "bg-white/[0.03] border border-white/[0.08] rounded-md text-[var(--text-primary)] placeholder:text-[var(--text-muted)] transition-colors duration-150 focus:border-[var(--accent-primary)]/40 focus:outline-none h-10 px-3";
  const selectClasses = "w-full h-10 rounded-md border px-3 text-sm bg-white/[0.03] border-white/[0.08] text-[var(--text-primary)] transition-colors duration-150 focus:outline-none focus:border-[var(--accent-primary)]/40";

  return (
    <Dialog open={!!proposal} onOpenChange={handleOpenChange}>
      <DialogContent
        data-testid="proposal-edit-modal"
        className="max-w-3xl !flex !flex-col overflow-hidden"
        style={{ maxHeight: "85vh" }}
        aria-labelledby="modal-title"
        aria-describedby="modal-description"
      >
        {/* Header */}
        <DialogHeader className="flex-shrink-0">
          <div className="flex items-center gap-4">
            <div className="w-10 h-10 rounded-xl bg-[var(--accent-primary)]/10 flex items-center justify-center flex-shrink-0">
              <Edit3 className="w-5 h-5 text-[var(--accent-primary)]" />
            </div>
            <div>
              <DialogTitle id="modal-title" className="text-lg font-medium">
                Edit Proposal
              </DialogTitle>
              <p id="modal-description" className="text-sm text-[var(--text-muted)] mt-0.5">
                Refine the details of your task proposal
              </p>
            </div>
          </div>
        </DialogHeader>

        {/* Scrollable Content */}
        <div className="flex-1 min-h-0 overflow-y-auto px-6">
          <div className="py-6 space-y-8">
            {/* Title */}
            <div className="space-y-2">
              <Label htmlFor="proposal-title" className="text-sm font-medium text-[var(--text-secondary)]">
                Title
              </Label>
              <Input
                ref={titleInputRef}
                id="proposal-title"
                type="text"
                value={title}
                onChange={(e) => setTitle(e.target.value)}
                className={inputClasses}
                disabled={isSaving}
                placeholder="What needs to be done?"
              />
            </div>

            {/* Description */}
            <div className="space-y-2">
              <Label htmlFor="proposal-description" className="text-sm font-medium text-[var(--text-secondary)]">
                Description
              </Label>
              <Textarea
                id="proposal-description"
                value={description}
                onChange={(e) => setDescription(e.target.value)}
                rows={3}
                className={`${inputClasses} resize-none h-auto py-2.5`}
                disabled={isSaving}
                placeholder="Describe the task in detail..."
              />
            </div>

            {/* Metadata Row */}
            <div className="grid grid-cols-3 gap-6">
              {/* Category */}
              <div className="space-y-2">
                <Label htmlFor="proposal-category" className="text-sm font-medium text-[var(--text-secondary)]">
                  Category
                </Label>
                <select
                  id="proposal-category"
                  value={category}
                  onChange={(e) => setCategory(e.target.value)}
                  className={selectClasses}
                  disabled={isSaving}
                >
                  {CATEGORIES.map((cat) => (
                    <option key={cat.value} value={cat.value}>
                      {cat.label}
                    </option>
                  ))}
                </select>
              </div>

              {/* Priority */}
              <div className="space-y-2">
                <Label htmlFor="proposal-priority" className="text-sm font-medium text-[var(--text-secondary)]">
                  Priority
                </Label>
                <select
                  id="proposal-priority"
                  value={userPriority}
                  onChange={(e) => setUserPriority(e.target.value as Priority | "")}
                  className={selectClasses}
                  disabled={isSaving}
                >
                  {PRIORITIES.map((p) => (
                    <option key={p.value || "auto"} value={p.value}>
                      {p.value === "" ? `Auto (${proposal.suggestedPriority})` : p.label}
                    </option>
                  ))}
                </select>
              </div>

              {/* Complexity */}
              <div className="space-y-2">
                <Label className="text-sm font-medium text-[var(--text-secondary)]">
                  Complexity
                </Label>
                <div className="h-10 flex items-center">
                  <ComplexitySelector
                    value={complexity}
                    onChange={setComplexity}
                    disabled={isSaving}
                  />
                </div>
              </div>
            </div>

            {/* Implementation Steps */}
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-[var(--text-secondary)]">
                  Implementation Steps
                </Label>
                <span className="text-xs text-[var(--text-muted)]">
                  {steps.length} step{steps.length !== 1 ? "s" : ""}
                </span>
              </div>

              <div className="space-y-2">
                {steps.map((step, index) => (
                  <div key={index} className="group flex items-center gap-3">
                    <span className="text-sm text-[var(--accent-primary)] font-mono w-6 flex-shrink-0 text-right">
                      {index + 1}.
                    </span>
                    <Input
                      data-testid="step-input"
                      type="text"
                      value={step}
                      onChange={(e) => handleStepChange(index, e.target.value)}
                      aria-label={`Step ${index + 1}`}
                      className={`${inputClasses} flex-1 h-9 text-sm`}
                      disabled={isSaving}
                      placeholder="Describe this step..."
                    />
                    <button
                      type="button"
                      onClick={() => handleRemoveStep(index)}
                      aria-label={`Remove step ${index + 1}`}
                      disabled={isSaving}
                      className="w-8 h-8 rounded flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity text-[var(--text-muted)] hover:text-status-error hover:bg-status-error/10"
                    >
                      <X className="w-4 h-4" />
                    </button>
                  </div>
                ))}

                <button
                  type="button"
                  onClick={handleAddStep}
                  disabled={isSaving}
                  className="w-full h-10 rounded-md border border-dashed border-[var(--border-subtle)] hover:border-[var(--accent-border)] text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors flex items-center justify-center gap-2 text-sm"
                >
                  <Plus className="w-4 h-4" />
                  Add step
                </button>
              </div>
            </div>

            {/* Acceptance Criteria */}
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <Label className="text-sm font-medium text-[var(--text-secondary)]">
                  Acceptance Criteria
                </Label>
                <span className="text-xs text-[var(--text-muted)]">
                  {acceptanceCriteria.length} criteri{acceptanceCriteria.length !== 1 ? "a" : "on"}
                </span>
              </div>

              <div className="space-y-2">
                {acceptanceCriteria.map((criterion, index) => (
                  <div key={index} className="group flex items-center gap-3">
                    <div className="w-6 flex justify-center flex-shrink-0">
                      <div className="w-4 h-4 rounded border border-[var(--accent-primary)]/40 flex items-center justify-center">
                        <div className="w-2 h-2 rounded-sm bg-[var(--accent-primary)]/60" />
                      </div>
                    </div>
                    <Input
                      data-testid="criterion-input"
                      type="text"
                      value={criterion}
                      onChange={(e) => handleCriterionChange(index, e.target.value)}
                      aria-label={`Acceptance criterion ${index + 1}`}
                      className={`${inputClasses} flex-1 h-9 text-sm`}
                      disabled={isSaving}
                      placeholder="Define success condition..."
                    />
                    <button
                      type="button"
                      onClick={() => handleRemoveCriterion(index)}
                      aria-label={`Remove criterion ${index + 1}`}
                      disabled={isSaving}
                      className="w-8 h-8 rounded flex items-center justify-center opacity-0 group-hover:opacity-100 transition-opacity text-[var(--text-muted)] hover:text-status-error hover:bg-status-error/10"
                    >
                      <X className="w-4 h-4" />
                    </button>
                  </div>
                ))}

                <button
                  type="button"
                  onClick={handleAddCriterion}
                  disabled={isSaving}
                  className="w-full h-10 rounded-md border border-dashed border-[var(--border-subtle)] hover:border-[var(--accent-border)] text-[var(--text-muted)] hover:text-[var(--text-secondary)] transition-colors flex items-center justify-center gap-2 text-sm"
                >
                  <Plus className="w-4 h-4" />
                  Add criterion
                </button>
              </div>
            </div>
          </div>
        </div>

        {/* Footer */}
        <DialogFooter className="flex-shrink-0">
          <Button
            data-testid="cancel-button"
            variant="ghost"
            onClick={onCancel}
            disabled={isSaving}
            className="text-[var(--text-secondary)] hover:text-[var(--text-primary)] hover:bg-white/[0.04]"
          >
            Cancel
          </Button>
          <Button
            data-testid="confirm-button"
            onClick={handleSave}
            disabled={!canSave}
            className="bg-[var(--accent-primary)] hover:bg-[var(--accent-primary)]/90 text-white px-6"
          >
            {isSaving && <Loader2 className="w-4 h-4 mr-2 animate-spin" />}
            {isSaving ? "Saving..." : "Save Changes"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
