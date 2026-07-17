import { z } from "zod";

import { typedInvoke, typedInvokeWithTransform } from "@/lib/tauri";

import {
  ComposerRoleDefaultResponseSchema,
  ManualRoleCatalogResponseSchema,
  ManualRoleDefaultResponseSchema,
} from "./manual-role-defaults.schemas";
import {
  transformComposerRoleDefault,
  transformManualRoleCatalog,
  transformManualRoleDefault,
} from "./manual-role-defaults.transforms";
import type {
  AgentConversationRoleDefaultInput,
  ClearManualRoleDefaultInput,
  ComposerRoleDefault,
  ManualRoleCatalog,
  ManualRoleDefault,
  StartComposerRoleDefaultInput,
  UpdateManualRoleDefaultInput,
} from "./manual-role-defaults.types";

export const manualRoleDefaultsApi = {
  list(projectId: string | null): Promise<ManualRoleCatalog> {
    return typedInvokeWithTransform(
      "get_manual_role_defaults",
      { projectId },
      ManualRoleCatalogResponseSchema,
      transformManualRoleCatalog,
    );
  },

  update(input: UpdateManualRoleDefaultInput): Promise<ManualRoleDefault> {
    return typedInvokeWithTransform(
      "update_manual_role_default",
      { input },
      ManualRoleDefaultResponseSchema,
      transformManualRoleDefault,
    );
  },

  clear(input: ClearManualRoleDefaultInput): Promise<boolean> {
    return typedInvoke("clear_manual_role_default", { input }, z.boolean());
  },

  getStartComposerDefault(
    input: StartComposerRoleDefaultInput,
  ): Promise<ComposerRoleDefault> {
    return typedInvokeWithTransform(
      "get_start_composer_role_default",
      { input },
      ComposerRoleDefaultResponseSchema,
      transformComposerRoleDefault,
    );
  },

  getConversationDefault(
    input: AgentConversationRoleDefaultInput,
  ): Promise<ComposerRoleDefault> {
    return typedInvokeWithTransform(
      "get_agent_conversation_role_default",
      { input },
      ComposerRoleDefaultResponseSchema,
      transformComposerRoleDefault,
    );
  },

  resetConversation(
    input: AgentConversationRoleDefaultInput,
  ): Promise<ComposerRoleDefault> {
    return typedInvokeWithTransform(
      "reset_agent_conversation_role_default",
      { input },
      ComposerRoleDefaultResponseSchema,
      transformComposerRoleDefault,
    );
  },
} as const;
