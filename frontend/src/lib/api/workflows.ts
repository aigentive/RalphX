/**
 * Tauri API wrappers for workflow operations
 *
 * Provides type-safe functions for workflow CRUD with Zod validation.
 * Uses snake_case response schemas from @/types/workflow and applies
 * transforms to return camelCase display types.
 */

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { InternalStatusSchema } from "@/types/status";
import {
  WorkflowResponseSchema,
  WorkflowColumnResponseSchema,
  ExternalSyncConfigResponseSchema,
  transformWorkflow,
  transformWorkflowColumn,
  type WorkflowSchema,
  type WorkflowColumn,
} from "@/types/workflow";

// Re-export schemas for consumers that import from this module
export { WorkflowResponseSchema, WorkflowColumnResponseSchema };

// Backward-compatible type aliases (these are now camelCase display types)
export type WorkflowResponse = WorkflowSchema;
export type WorkflowColumnResponse = WorkflowColumn;

// ============================================================================
// Internal Response Schema Arrays (for parsing lists)
// ============================================================================

const WorkflowListResponseSchema = z.array(WorkflowResponseSchema);
const ColumnListResponseSchema = z.array(WorkflowColumnResponseSchema);

// ============================================================================
// Input Schemas (for validating client-side input before sending)
// ============================================================================

/**
 * Schema for column input when creating/updating workflows
 */
export const WorkflowColumnInputSchema = z.object({
  id: z.string(),
  name: z.string(),
  maps_to: InternalStatusSchema,
  color: z.string().optional(),
  icon: z.string().optional(),
  skip_review: z.boolean().optional(),
  auto_advance: z.boolean().optional(),
  agent_profile: z.string().optional(),
});

export type WorkflowColumnInput = z.infer<typeof WorkflowColumnInputSchema>;

/**
 * Schema for creating a new workflow
 */
export const CreateWorkflowInputSchema = z.object({
  name: z.string().min(1),
  description: z.string().optional(),
  columns: z.array(WorkflowColumnInputSchema).min(1),
  is_default: z.boolean().optional(),
  worker_profile: z.string().optional(),
  reviewer_profile: z.string().optional(),
  external_sync: ExternalSyncConfigResponseSchema.optional(),
});

export type CreateWorkflowInput = z.infer<typeof CreateWorkflowInputSchema>;

/**
 * Schema for updating an existing workflow (all fields optional)
 */
export const UpdateWorkflowInputSchema = z.object({
  name: z.string().min(1).optional(),
  description: z.string().optional(),
  columns: z.array(WorkflowColumnInputSchema).min(1).optional(),
  is_default: z.boolean().optional(),
  worker_profile: z.string().optional(),
  reviewer_profile: z.string().optional(),
  external_sync: ExternalSyncConfigResponseSchema.optional(),
});

export type UpdateWorkflowInput = z.infer<typeof UpdateWorkflowInputSchema>;

// ============================================================================
// API Functions
// ============================================================================

/**
 * List all workflows
 * @returns Array of workflows (camelCase display types)
 */
export async function getWorkflows(): Promise<WorkflowSchema[]> {
  const result = await invoke("get_workflows", {});
  const parsed = WorkflowListResponseSchema.parse(result);
  return parsed.map(transformWorkflow);
}

/**
 * Get a single workflow by ID
 * @param id The workflow ID
 * @returns The workflow or null if not found (camelCase display type)
 */
export async function getWorkflow(id: string): Promise<WorkflowSchema | null> {
  const result = await invoke("get_workflow", { id });
  const parsed = WorkflowResponseSchema.nullable().parse(result);
  return parsed ? transformWorkflow(parsed) : null;
}

/**
 * Create a new workflow
 * @param input Workflow creation data
 * @returns The created workflow (camelCase display type)
 * @throws ZodError if input validation fails
 */
export async function createWorkflow(input: CreateWorkflowInput): Promise<WorkflowSchema> {
  // Validate input before sending
  const validatedInput = CreateWorkflowInputSchema.parse(input);
  const result = await invoke("create_workflow", { input: validatedInput });
  const parsed = WorkflowResponseSchema.parse(result);
  return transformWorkflow(parsed);
}

/**
 * Update an existing workflow
 * @param id The workflow ID
 * @param input Partial workflow data to update
 * @returns The updated workflow (camelCase display type)
 * @throws ZodError if input validation fails
 */
export async function updateWorkflow(
  id: string,
  input: UpdateWorkflowInput
): Promise<WorkflowSchema> {
  // Validate input before sending
  const validatedInput = UpdateWorkflowInputSchema.parse(input);
  const result = await invoke("update_workflow", { id, input: validatedInput });
  const parsed = WorkflowResponseSchema.parse(result);
  return transformWorkflow(parsed);
}

/**
 * Set a workflow as the default
 * @param id The workflow ID to set as default
 * @returns The updated workflow (camelCase display type)
 */
export async function setDefaultWorkflow(id: string): Promise<WorkflowSchema> {
  const result = await invoke("set_default_workflow", { id });
  const parsed = WorkflowResponseSchema.parse(result);
  return transformWorkflow(parsed);
}

/**
 * Get the columns for the currently active/default workflow
 * @returns Array of workflow columns (camelCase display types)
 */
export async function getActiveWorkflowColumns(): Promise<WorkflowColumn[]> {
  const result = await invoke("get_active_workflow_columns", {});
  const parsed = ColumnListResponseSchema.parse(result);
  return parsed.map(transformWorkflowColumn);
}

/**
 * Get the built-in workflow definitions
 * @returns Array of built-in workflows (camelCase display types)
 */
export async function getBuiltinWorkflows(): Promise<WorkflowSchema[]> {
  const result = await invoke("get_builtin_workflows", {});
  const parsed = WorkflowListResponseSchema.parse(result);
  return parsed.map(transformWorkflow);
}
